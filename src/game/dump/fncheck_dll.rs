// Hand-written free-function twins checked against the game's DLL (tools/dllcheck): methods the game runs outside the
// engine whose logic the rewrite holds in a constant, a match arm or a helper. Rows come from DllCheck's fnpure lines.
use super::super::blood_wire::WireKind;
use super::super::poison::PoisonKind;
use super::super::syringe::should_call_on_enter_every_update as on_enter;

fn dll_game() -> super::super::Game {
    super::super::Game::default()
}
fn dll_use_channel(tool: &str) -> u16 {
    match WireKind::of_tool(tool) {
        Some(WireKind::Use(channel, _)) => channel,
        k => panic!("{tool} is {k:?}"),
    }
}

// Methods run on an object (DllCheck's objects mode): args[0] is the object as recorded, `Type(F=v ...)`; the row's ret is
// the return value, then `set(F=v ...)`, the fields the call changed.
fn dll_self_field(c: &Case, name: &str) -> Result<String, String> {
    dll_record_field(c.args.first().ok_or("no self")?, name)
}
/// A field of a component the self links to (`F=@name#id`), as DllCheck loaded it: the self's `@link#id=Type(..)`.
fn dll_link_field(c: &Case, link: &str, name: &str) -> Result<String, String> {
    let r = dll_self_field(c, link)?;
    let id = r.rsplit('#').next().filter(|_| r.starts_with('@')).ok_or_else(|| format!("{link} is {r}"))?;
    dll_record_field(&dll_self_field(c, &format!("@link#{id}"))?, name)
}
/// The self's linked component of type `ty` (`@link#id=Type(..)`): its id and record.
fn dll_link_of_type(c: &Case, ty: &str) -> Result<(String, String), String> {
    let s = c.args.first().ok_or("no self")?;
    let at = s.find(&format!("={ty}(")).and_then(|i| s[..i].rfind(" @link#").map(|j| (j, i))).ok_or_else(|| format!("no linked {ty}"))?;
    let id = s[at.0 + 7..at.1].to_string();
    Ok((id.clone(), dll_self_field(c, &format!("@link#{id}"))?))
}
fn dll_record_field(s: &str, name: &str) -> Result<String, String> {
    let inner = &s[s.find('(').ok_or("self not a record")? + 1..s.len() - 1];
    let (mut depth, mut at) = (0i32, 0);
    let mut parts = Vec::new();
    for (i, ch) in inner.char_indices() {
        match ch {
            '[' | '{' | '(' => depth += 1,
            ']' | '}' | ')' => depth -= 1,
            ' ' if depth == 0 => { parts.push(&inner[at..i]); at = i + 1 }
            _ => {}
        }
    }
    parts.push(&inner[at..]);
    parts.iter().find_map(|kv| kv.strip_prefix(name).and_then(|v| v.strip_prefix('='))).map(str::to_string).ok_or_else(|| format!("self has no {name}"))
}
fn dll_floats<const N: usize>(c: &Case, name: &str) -> Result<[f32; N], String> {
    let v = dll_self_field(c, name)?;
    let mut out = [0.0; N];
    for (i, w) in v.split(',').take(N).enumerate() {
        out[i] = u32::from_str_radix(w, 16).map(f32::from_bits).map_err(|_| format!("{name} not floats: {v}"))?;
    }
    Ok(out)
}
fn dll_f32(c: &Case, name: &str) -> Result<f32, String> {
    Ok(dll_floats::<1>(c, name)?[0])
}
fn dll_i32(c: &Case, name: &str) -> Result<i32, String> {
    str::parse::<i32>(&dll_self_field(c, name)?).map_err(|_| format!("{name} not an int"))
}
/// A Vector2Int field: `Vector2Int(m_X=a m_Y=b)`.
fn dll_v2i(c: &Case, name: &str) -> Result<[i32; 2], String> {
    let v = dll_self_field(c, name)?;
    let x = v.split("m_X=").nth(1).and_then(|r| r.split(' ').next()).ok_or("no m_X")?;
    let y = v.split("m_Y=").nth(1).and_then(|r| r.split(')').next()).ok_or("no m_Y")?;
    Ok([str::parse::<i32>(x).map_err(|_| "m_X")?, str::parse::<i32>(y).map_err(|_| "m_Y")?])
}
/// A string field (the recording writes a space as `_`: taken as written, the names read this way have no spaces).
fn dll_string(c: &Case, name: &str) -> Result<String, String> {
    Ok(dll_self_field(c, name)?.trim_matches('"').to_string())
}
/// A method that changes nothing: its return value and an empty change list.
fn dll_ret(v: impl Enc) -> String {
    format!("{} set()", v.enc())
}
fn dll_map_bounds(c: &Case) -> Result<super::super::map_editor::view::MapBounds, String> {
    Ok(super::super::map_editor::view::MapBounds { BoundsCenterInMeters: dll_v2i(c, "BoundsCenterInMeters")?, BoundsSizeInMeters: dll_v2i(c, "BoundsSizeInMeters")? })
}
fn dll_rect(c: &Case) -> Result<super::super::value_types::RotatedRectangle, String> {
    Ok(super::super::value_types::RotatedRectangle::new(dll_floats(c, "Center")?, dll_floats(c, "Size")?, dll_floats(c, "Pivot")?, dll_f32(c, "AngleDegrees")?))
}
fn dll_segment(c: &Case) -> Result<super::super::value_types::LineSegment, String> {
    Ok(super::super::value_types::LineSegment::new(dll_floats(c, "A")?, dll_floats(c, "B")?))
}
fn dll_interval(c: &Case) -> Result<super::super::misc::FixedIntervalDistributor, String> {
    Ok(super::super::misc::FixedIntervalDistributor {
        timeStepAccumulator: dll_f32(c, "timeStepAccumulator")?,
        MaxCycles: dll_i32(c, "<MaxCycles>k__BackingField")?,
        InterpolationTime: dll_f32(c, "<InterpolationTime>k__BackingField")?,
        IntervalSeconds: dll_f32(c, "<IntervalSeconds>k__BackingField")?,
    })
}
/// A script's fields and Field links from the self record (an engine object reference, `@name#id`, left as is), and what a
/// call changed: `set(F=v ...)` by name, as DllCheck lists the game's.
macro_rules! dll_load {
    ($c:expr, $s:expr) => {{
        let (f, l) = $s.fields_and_links();
        for (n, f) in f.into_iter().chain(l) {
            if let Ok(v) = dll_self_field($c, n) {
                // A bool link is the reference's presence: `@name#id` set, `null` not.
                let bool_link = matches!(f.show().as_str(), "0" | "1");
                if bool_link && (v == "null" || v.starts_with('@')) {
                    f.parse(if v == "null" { "0" } else { "1" }).map_err(|e| format!("{n}: {e}"))?;
                } else if !v.starts_with('@') {
                    f.parse(&v).map_err(|e| format!("{n}: {e}"))?;
                }
            }
        }
    }};
}
macro_rules! dll_show {
    ($s:expr) => {{
        let (f, l) = $s.fields_and_links();
        f.into_iter().chain(l).map(|(n, f)| (n, f.show())).collect::<Vec<_>>()
    }};
}
/// A DeltaInt method on the recorded one: the fields it changed.
fn dll_delta_int(c: &Case, call: impl FnOnce(&mut super::super::continuous::DeltaInt) -> Result<(), String>) -> Result<String, String> {
    let mut d = super::super::continuous::DeltaInt { Value: dll_i32(c, "Value")?, PreviousValue: dll_i32(c, "PreviousValue")?, PreviousValueFixed: dll_i32(c, "PreviousValueFixed")? };
    let show = |d: &super::super::continuous::DeltaInt| [("PreviousValue", d.PreviousValue.to_string()), ("PreviousValueFixed", d.PreviousValueFixed.to_string()), ("Value", d.Value.to_string())];
    let before = show(&d);
    call(&mut d)?;
    Ok(format!("void {}", dll_changes(&before, &show(&d))))
}
/// A Game holding the recorded LiquidContainerController's FillPercentage and ParticleRadius (container 0).
fn dll_liquid_container(c: &Case) -> Result<super::super::Game, String> {
    let mut g = dll_game();
    let mut s = super::super::flask::LiquidContainer::default();
    s.FillPercentage = dll_f32(c, "FillPercentage")?;
    s.ParticleRadius = dll_f32(c, "ParticleRadius")?;
    g.liquid_containers.push(s);
    Ok(g)
}
/// gapA r20: the recorded PhysicalBehaviour (`rec`) as physicals[0] with its audioSource (an AudioSource on an active
/// object, disabled before, as DllCheck's stand-in) and isDisintegrated; returns the source's `@name#id` and id.
fn dll_physical_audio(g: &mut super::super::Game, rec: &str) -> Result<(String, Option<i32>), String> {
    let r = dll_rec(rec)?;
    let source = dll_get(&r, "audioSource")?.to_string();
    let sid = if source == "null" { None } else { Some(reference(&source).ok_or("audioSource")?) };
    let mut p = super::super::Physical::default();
    p.isDisintegrated = dll_get(&r, "isDisintegrated")? == "1";
    p.audioSource = sid.is_some();
    p.ids.audioSource = sid.unwrap_or(0);
    if let Some(id) = sid {
        g.audio.add(id, super::super::audio::AudioSource { active: true, enabled: false, ..Default::default() });
    }
    g.physicals.push(p);
    Ok((source, sid))
}
/// gapA r20: component `id`'s single AudioClip field `name` as recorded (`@name#id` or null), a clip named by it.
fn dll_clip_field(g: &mut super::super::Game, id: i32, name: &str, v: &str) {
    let clip = (v != "null").then(|| std::sync::Arc::new(super::super::audio::Clip {
        asset: Default::default(),
        clip: crate::assets::classes::AudioClip { name: v.to_string(), load_type: 0, channels: 1, frequency: 44100, length: 1.0, resource: Default::default(), compression_format: 0 },
        pcm: None,
        stream: None,
    }));
    g.audio.clip_fields.insert((id, name.to_string()), vec![clip]);
}
/// gapA r20: what a call did to source `sid`, as DllCheck lists it after the field changes: `!enabled(@source)=1` when it
/// is now enabled, then `!PlayOneShot(@source,@clip,volume)` per one-shot.
fn dll_source_fx(g: &super::super::Game, sid: Option<i32>, source: &str) -> Vec<String> {
    let mut fx = Vec::new();
    if let Some(s) = sid.and_then(|id| g.audio.sources.get(&id)) {
        if s.enabled { fx.push(format!("!enabled({source})=1")) }
        for v in &s.one_shots { fx.push(format!("!PlayOneShot({source},{},{})", v.clip.clip.name, hex(v.volume_scale))) }
    }
    fx
}
/// `set(a b)` with the effects appended inside: `set(a b fx..)`.
fn dll_with_fx(set: &str, fx: &[String]) -> String {
    let inner = &set[4..set.len() - 1];
    let all: Vec<String> = inner.split(' ').filter(|s| !s.is_empty()).map(String::from).chain(fx.iter().cloned()).collect();
    format!("set({})", all.join(" "))
}
fn dll_changes(before: &[(&str, String)], after: &[(&str, String)]) -> String {
    let mut out: Vec<String> = after.iter().zip(before).filter(|(a, b)| a.1 != b.1).map(|(a, _)| format!("{}={}", a.0, a.1)).collect();
    out.sort();
    format!("set({})", out.join(" "))
}
/// A ReactorCoreBehaviour method on the recorded core (the only one in a default Game, id 1).
fn dll_reactor(c: &Case, n: usize, call: impl FnOnce(&mut super::super::Game, &With) -> Result<String, String>) -> Result<String, String> {
    let w = c.with(n)?;
    let mut g = dll_game();
    let mut s = super::super::reactor::ReactorCore::default();
    dll_load!(c, s);
    s.id = 1;
    let before = dll_show!(s);
    g.reactor_cores.push(s);
    let ret = call(&mut g, &w)?;
    let after = dll_show!(g.reactor_cores[0]);
    Ok(format!("{ret} {}", dll_changes(&before, &after)))
}
fn dll_reactor_call(c: &Case, call: super::super::reactor::ReactorCall) -> Result<String, String> {
    dll_reactor(c, 1, |g, _| { g.reactor_core_call(1, call); Ok("void".into()) })
}
/// FixedIntervalDistributor.set_RateHz/set_Rate: the interval it sets, listed when it changed.
fn dll_set_rate(c: &Case) -> Result<String, String> {
    let w = c.with(2)?;
    let mut f = dll_interval(c)?;
    let before = f.IntervalSeconds;
    f.set_RateHz(w.p_f32(1)?);
    let changed = if f.IntervalSeconds.to_bits() != before.to_bits() { format!("<IntervalSeconds>k__BackingField={}", hex(f.IntervalSeconds)) } else { String::new() };
    Ok(format!("void set({changed})"))
}

/// The component's GameObject as DllCheck's engine stand-ins saw it (the `@` fields): a PhysicalBehaviour with the recorded
/// world position and rotation (as a root: no parent chain) and local scale.
fn dll_physical(c: &Case) -> Result<super::super::Physical, String> {
    let mut p = super::super::Physical::default();
    p.transform.position = dll_floats(c, "@pos")?;
    p.transform.rotation = dll_floats(c, "@rot")?;
    p.transform.localScale = dll_floats(c, "@lscale")?;
    Ok(p)
}
/// A translated script on the recorded object, its PhysicalBehaviour physicals[0].
fn dll_on_physical(c: &Case) -> Result<super::super::Game, String> {
    let mut g = dll_game();
    g.physicals.push(dll_physical(c)?);
    Ok(g)
}

/// UnityEngine.Random as DllCheck seeded it for the call (`@rng`, the xorshift128 state).
fn dll_random(c: &Case) -> Result<crate::unity::Random, String> {
    let v = dll_self_field(c, "@rng")?;
    let mut state = [0u32; 4];
    for (i, w) in v.split(',').enumerate().take(4) {
        state[i] = u32::from_str_radix(w, 16).map_err(|_| format!("@rng: {v}"))?;
    }
    Ok(crate::unity::Random { state })
}

/// Grid`1 on DllCheck's grid rows (the game's Grid<int>): a grid arg is W;H;[Flat] as the game held it; ret is the return
/// value (out params comma-joined after it) or "throw" when the call threw (here: panicked), then ` flat=` the Flat after.
fn dll_grid(c: &Case, m: &str) -> Result<String, String> {
    use super::super::grid::Grid;
    let a = &c.args;
    let int = |i: usize| a.get(i).ok_or("arity")?.parse::<i32>().map_err(|_| format!("arg {i} not an int"));
    let list = |i: usize| -> Result<Vec<i32>, String> {
        let s = a.get(i).ok_or("arity")?.trim_matches(|x| x == '[' || x == ']');
        if s.is_empty() { Ok(Vec::new()) } else { s.split(';').map(|v| v.parse::<i32>().map_err(|_| format!("arg {i} not ints"))).collect() }
    };
    let grid = |i: usize| -> Result<Grid<i32>, String> { Ok(Grid { Width: int(i)?, Height: int(i + 1)?, Flat: list(i + 2)? }) };
    let l = |f: &[i32]| format!("[{}]", f.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(";"));
    let s = |g: &Grid<i32>| format!("{};{};{}", g.Width, g.Height, l(&g.Flat));
    let items = |g: &Grid<i32>| format!("[{}]", g.iter().map(|(x, y, v)| format!("{x},{y},{v}")).collect::<Vec<_>>().join(";"));
    // A panic is the game's exception: caught quietly (the hook would print one line per case).
    let quiet = |f: &mut dyn FnMut() -> String| {
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| "throw".into());
        std::panic::set_hook(hook);
        r
    };
    Ok(match (m, a.len()) {
        ("constructor", 2) => { let (w, h) = (int(0)?, int(1)?); quiet(&mut || s(&Grid::<i32>::new(w, h))) }
        ("constructor", 3) => { let (w, h, f) = (int(0)?, int(1)?, list(2)?); quiet(&mut || s(&Grid::from_flat(w, h, f.clone()))) }
        ("Get", 5) => { let (g, x, y) = (grid(0)?, int(3)?, int(4)?); quiet(&mut || g.get(x, y).to_string()) }
        ("TryGet", 5) => { let (g, x, y) = (grid(0)?, int(3)?, int(4)?); quiet(&mut || match g.try_get(x, y) { Some(v) => format!("1,{v}"), None => "0,0".into() }) }
        ("GetIndexFrom", 5) => { let (g, x, y) = (grid(0)?, int(3)?, int(4)?); quiet(&mut || g.get_index_from(x, y).to_string()) }
        ("Set", 6) => {
            let (mut g, x, y, v) = (grid(0)?, int(3)?, int(4)?, int(5)?);
            let r = quiet(&mut || { g.set(x, y, v); "void".into() });
            format!("{r} flat={}", l(&g.Flat))
        }
        ("IndexFromCoordinates", 3) => { let (x, y, w) = (int(0)?, int(1)?, int(2)?); quiet(&mut || Grid::<i32>::index_from_coordinates(x, y, w).to_string()) }
        ("GetAt", 4) => { let (f, x, y, w) = (list(0)?, int(1)?, int(2)?, int(3)?); quiet(&mut || Grid::get_at(&f, x, y, w).to_string()) }
        ("SetAt", 5) => {
            let (mut f, x, y, w, v) = (list(0)?, int(1)?, int(2)?, int(3)?, int(4)?);
            let r = quiet(&mut || { Grid::set_at(&mut f, x, y, w, v); "void".into() });
            format!("{r} flat={}", l(&f))
        }
        ("GetCoordinatesFromIndex", 4) => { let (g, i) = (grid(0)?, int(3)?); quiet(&mut || { let (x, y) = g.get_coordinates_from_index(i); format!("{x},{y}") }) }
        ("GetCoordinatesFromIndex", 2) => { let (i, w) = (int(0)?, int(1)?); quiet(&mut || { let (x, y) = Grid::<i32>::coordinates_from_index(i, w); format!("{x},{y}") }) }
        ("GetEnumerator", 3) => { let g = grid(0)?; quiet(&mut || items(&g)) }
        ("System.Collections.IEnumerable.GetEnumerator", 3) => {
            let g = grid(0)?;
            quiet(&mut || format!("[{}]", g.enumerable_get_enumerator().map(|(x, y, v)| format!("{x},{y},{v}")).collect::<Vec<_>>().join(";")))
        }
        ("BlockEquals", 8) => { let (g, b, t, le) = (grid(0)?, grid(3)?, int(6)?, int(7)?); quiet(&mut || bit(g.block_equals(&b, t, le))) }
        ("WriteBlock", 8) => {
            let (mut g, b, t, le) = (grid(0)?, grid(3)?, int(6)?, int(7)?);
            let r = quiet(&mut || { g.write_block(&b, t, le); "void".into() });
            format!("{r} flat={}", l(&g.Flat))
        }
        _ => return Err(format!("no Grid`1 twin for {m} with {} args", a.len())),
    })
}

// DllCheck's seeded rows (objects with seeded fields): args[0] the object `Type(F=v ...)`, then the args; the game's
// answer is `<ret or throw> set(<changed fields>) after=<args after the call>`.
/// A record's `F=v` parts: `Type(F=v F=v ...)`, split at the spaces outside brackets.
fn dll_rec(s: &str) -> Result<Vec<(&str, &str)>, String> {
    let inner = &s[s.find('(').ok_or("not a record")? + 1..s.len().checked_sub(1).ok_or("not a record")?];
    let (mut depth, mut at) = (0i32, 0);
    let mut parts = Vec::new();
    for (i, ch) in inner.char_indices() {
        match ch {
            '[' | '{' | '(' => depth += 1,
            ']' | '}' | ')' => depth -= 1,
            ' ' if depth == 0 => { parts.push(&inner[at..i]); at = i + 1 }
            _ => {}
        }
    }
    parts.push(&inner[at..]);
    Ok(parts.into_iter().filter_map(|kv| kv.split_once('=')).collect())
}
fn dll_get<'a>(rec: &[(&str, &'a str)], name: &str) -> Result<&'a str, String> {
    rec.iter().find(|(n, _)| *n == name).map(|(_, v)| *v).ok_or_else(|| format!("no field {name}"))
}
fn dll_bits(v: &str) -> Result<f32, String> {
    u32::from_str_radix(v, 16).map(f32::from_bits).map_err(|_| format!("not a float: {v}"))
}
/// `[a;b;...]` split at the top-level `;`.
fn dll_items(v: &str) -> Result<Vec<&str>, String> {
    let s = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')).ok_or_else(|| format!("not a list: {v}"))?;
    Ok(if s.is_empty() { Vec::new() } else { split_args(s) })
}
fn dll_ints(v: &str) -> Result<Vec<i32>, String> {
    // Pushed one by one: a List<int> filled by Add has the same backing size (4, 8, 16, ...) as a Vec grown by push.
    let mut out = Vec::new();
    for x in dll_items(v)? {
        out.push(x.parse::<i32>().map_err(|_| format!("not an int: {x}"))?);
    }
    Ok(out)
}
fn dll_list<T: std::fmt::Display>(v: impl IntoIterator<Item = T>) -> String {
    format!("[{}]", v.into_iter().map(|x| x.to_string()).collect::<Vec<_>>().join(";"))
}
fn dll_hexes(v: &[f32]) -> String {
    dll_list(v.iter().map(|&x| hex(x)))
}
/// The answer row: ret, the changed fields (before/after by name, sorted as the game's), the args after.
fn dll_seeded_row(ret: String, before: &[(&str, String)], after: &[(&str, String)], args_after: &[String]) -> String {
    format!("{ret} {} after={}", dll_changes(before, after), args_after.join(";"))
}
/// gapC r28: DllCheck's seeded DecalInstruction (args[0]; type is a DecalDescriptor reference DllCheck leaves null).
fn dll_decal(c: &Case) -> Result<super::super::decal_renderer::DecalInstruction, String> {
    Ok(super::super::decal_renderer::DecalInstruction { globalPosition: dll_floats(c, "<globalPosition>k__BackingField")?, size: dll_f32(c, "<size>k__BackingField")?, colourMultiplier: dll_floats(c, "<colourMultiplier>k__BackingField")?, ..Default::default() })
}
/// A Vector2 / Color arg: N hex floats.
fn dll_vec<const N: usize>(s: &str) -> Result<[f32; N], String> {
    let v: Vec<f32> = s.split(',').map(dll_bits).collect::<Result<_, _>>()?;
    v.try_into().map_err(|_| format!("not {N} floats: {s}"))
}
/// gapC r25: a plain auto-property setter's row: the one backing field before and after, the arg after.
fn dll_prop_set(field: &str, before: String, after: String, arg: String) -> String {
    dll_seeded_row("void".into(), &[(field, before)], &[(field, after)], &[arg])
}
/// A panic is the game's exception: caught quietly.
fn dll_quiet<R>(f: impl FnOnce() -> R) -> Option<R> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok();
    std::panic::set_hook(hook);
    r
}

fn dll_env(rec: &[(&str, &str)]) -> Result<super::super::map::EnvironmentalSettings, String> {
    let b = |n: &str| -> Result<bool, String> { Ok(dll_get(rec, n)? == "1") };
    Ok(super::super::map::EnvironmentalSettings {
        Floodlights: b("Floodlights")?,
        Rain: b("Rain")?,
        Snow: b("Snow")?,
        Fog: b("Fog")?,
        Gravity: dll_bits(dll_get(rec, "Gravity")?)?,
        Lightning_chance: dll_bits(dll_get(rec, "Lightning_chance")?)?,
        Ambient_temperature: dll_bits(dll_get(rec, "Ambient_temperature")?)?,
    })
}
fn dll_env_text(e: &super::super::map::EnvironmentalSettings) -> String {
    format!(
        "EnvironmentalSettings(Floodlights={} Rain={} Snow={} Fog={} Gravity={} Lightning_chance={} Ambient_temperature={})",
        bit(e.Floodlights), bit(e.Rain), bit(e.Snow), bit(e.Fog), hex(e.Gravity), hex(e.Lightning_chance), hex(e.Ambient_temperature)
    )
}
fn dll_env_copy_to(c: &Case) -> Result<String, String> {
    c.with(2)?;
    let me = dll_env(&dll_rec(c.args[0])?)?;
    let mut other = dll_env(&dll_rec(c.args[1])?)?;
    me.copy_to(&mut other);
    Ok(dll_seeded_row("void".into(), &[], &[], &[dll_env_text(&other)]))
}

fn dll_ui_lock(c: &Case, register: bool) -> Result<String, String> {
    c.with(1)?;
    let rec = dll_rec(c.args[0])?;
    let mut e = super::super::map_editor::MapEditor::default();
    e.uiLockingRegistrations = dll_get(&rec, "uiLockingRegistrations")?.parse().map_err(|_| "uiLockingRegistrations")?;
    let before = [("uiLockingRegistrations", e.uiLockingRegistrations.to_string())];
    let ret = dll_quiet(|| if register { e.RegisterUILock() } else { e.DeregisterUILock() }).map_or("throw", |_| "void");
    Ok(dll_seeded_row(ret.into(), &before, &[("uiLockingRegistrations", e.uiLockingRegistrations.to_string())], &[]))
}

fn dll_lift_pop(c: &Case) -> Result<String, String> {
    let w = c.with(2)?;
    let rec = dll_rec(c.args[0])?;
    let mut g = dll_game();
    let mut l = super::super::map_lift::MapLift::default();
    l.Floors = dll_items(dll_get(&rec, "Floors")?)?.into_iter().map(dll_bits).collect::<Result<_, _>>()?;
    l.requestIndexList = dll_ints(dll_get(&rec, "requestIndexList")?)?;
    l.TargetFloorIndex = dll_get(&rec, "TargetFloorIndex")?.parse().map_err(|_| "TargetFloorIndex")?;
    let before = [("requestIndexList", dll_list(l.requestIndexList.iter()))];
    g.map_lifts.push(l);
    let h = w.p_f32(1)?;
    let ret = dll_quiet(|| g.map_lift_pop_nearest(0, h)).map_or("throw".into(), |v| v.to_string());
    Ok(dll_seeded_row(ret, &before, &[("requestIndexList", dll_list(g.map_lifts[0].requestIndexList.iter()))], &[hex(h)]))
}

fn dll_fire_points(m: &super::super::fire::FireMap) -> String {
    dll_list(m.Points.iter().map(|p| {
        let nb = if m.neighbours_null { "null".to_string() } else { dll_list(p.NeighbourIndices.iter()) };
        format!("BurnPoint(LocalPoint={},{} FireIntensity={} Softness={} NeighbourIndices={nb})", hex(p.LocalPoint[0]), hex(p.LocalPoint[1]), hex(p.FireIntensity), hex(p.Softness))
    }))
}
fn dll_fire(c: &Case, call: fn(&mut Game, usize)) -> Result<String, String> {
    c.with(1)?;
    let rec = dll_rec(c.args[0])?;
    let mut m = super::super::fire::FireMap::new(0);
    m.GridStep = dll_bits(dll_get(&rec, "GridStep")?)?;
    m.MergeDistanceMultiplier = dll_bits(dll_get(&rec, "MergeDistanceMultiplier")?)?;
    for p in dll_items(dll_get(&rec, "Points")?)? {
        let pr = dll_rec(p)?;
        let lp = dll_get(&pr, "LocalPoint")?.split_once(',').ok_or("LocalPoint")?;
        // gapA r20: a recorded state's bare "Int32[]" (the recorder's Format at depth 0) does not parse in DllCheck either: null there.
        let nb = match dll_get(&pr, "NeighbourIndices")? { "Int32[]" => "null", v => v };
        // Seeded maps are all null-neighboured (as CalculatePoints leaves them) or all not.
        m.neighbours_null = nb == "null";
        m.Points.push(super::super::fire::BurnPoint {
            LocalPoint: [dll_bits(lp.0)?, dll_bits(lp.1)?],
            FireIntensity: dll_bits(dll_get(&pr, "FireIntensity")?)?,
            Softness: dll_bits(dll_get(&pr, "Softness")?)?,
            NeighbourIndices: if nb == "null" { Vec::new() } else { dll_ints(nb)? },
        });
    }
    let before = [("Points", dll_fire_points(&m))];
    let mut g = dll_game();
    g.fire_maps.push(m);
    let ret = dll_quiet(|| call(&mut g, 0)).map_or("throw", |_| "void");
    Ok(dll_seeded_row(ret.into(), &before, &[("Points", dll_fire_points(&g.fire_maps[0]))], &[]))
}

const PURE_DLL: &[(&str, PureCall)] = &[
    ("EnvironmentalSettings.CopyTo", dll_env_copy_to),
    ("MapEditorGlobal.RegisterUILock", |c| dll_ui_lock(c, true)),
    ("MapEditorGlobal.DeregisterUILock", |c| dll_ui_lock(c, false)),
    ("MapLiftBehaviour.PopNextNearestTargetFloorIndex", dll_lift_pop),
    ("LocalFirePropagationMap.CalculateNeighbours", |c| dll_fire(c, |g, f| g.fire_map_calculate_neighbours(f))),
    ("LocalFirePropagationMap.MergeByDistance", |c| dll_fire(c, |g, f| g.fire_map_merge_by_distance(f))),
    ("MapLoadStep.Initialise", |c| { c.with(0)?; crate::game::background_loader::LaunchStep::initialise(&mut crate::ui::data::MapLoadStep::default(), &mut Vec::new()); Ok("void".into()) }),
    ("MapLoadStep.get_DisplayName", |c| { c.with(0)?; Ok(Enc::enc(crate::game::background_loader::LaunchStep::display_name(&crate::ui::data::MapLoadStep::default()))) }),
    ("MapLoadStep.get_Thumbnail", |c| { c.with(0)?; Ok(if crate::game::background_loader::LaunchStep::workshop_thumbnail(&crate::ui::data::MapLoadStep::default()) { "Sprite".into() } else { "null".into() }) }),
    ("Grid`1.constructor", |c| dll_grid(c, "constructor")),
    ("Grid`1.Get", |c| dll_grid(c, "Get")),
    ("Grid`1.TryGet", |c| dll_grid(c, "TryGet")),
    ("Grid`1.GetIndexFrom", |c| dll_grid(c, "GetIndexFrom")),
    ("Grid`1.Set", |c| dll_grid(c, "Set")),
    ("Grid`1.IndexFromCoordinates", |c| dll_grid(c, "IndexFromCoordinates")),
    ("Grid`1.GetAt", |c| dll_grid(c, "GetAt")),
    ("Grid`1.SetAt", |c| dll_grid(c, "SetAt")),
    ("Grid`1.GetCoordinatesFromIndex", |c| dll_grid(c, "GetCoordinatesFromIndex")),
    ("Grid`1.GetEnumerator", |c| dll_grid(c, "GetEnumerator")),
    ("Grid`1.System.Collections.IEnumerable.GetEnumerator", |c| dll_grid(c, "System.Collections.IEnumerable.GetEnumerator")),
    ("Grid`1.BlockEquals", |c| dll_grid(c, "BlockEquals")),
    ("Grid`1.WriteBlock", |c| dll_grid(c, "WriteBlock")),
    ("AOEPowerTool.get_DampeningRadius", |c| { c.with(0)?; Ok(Enc::enc(super::super::power::DAMPENING_RADIUS)) }),
    ("AOEPowerTool.get_Force", |c| { c.with(0)?; Ok(Enc::enc(super::super::power::FORCE)) }),
    ("AOEPowerTool.get_MaxForce", |c| { c.with(0)?; Ok(Enc::enc(super::super::power::MAX_FORCE)) }),
    ("AOEPowerTool.get_MouseMoveInfluence", |c| { c.with(0)?; Ok(Enc::enc(super::super::power::MOUSE_MOVE_INFLUENCE)) }),
    ("AOEPowerTool.get_Radius", |c| { c.with(0)?; Ok(Enc::enc(super::super::power::RADIUS)) }),
    ("AcidPoison.get_Lifespan", |c| { c.with(0)?; Ok(Enc::enc(PoisonKind::Acid.lifespan())) }),
    ("KnockoutPoison.get_Lifespan", |c| { c.with(0)?; Ok(Enc::enc(PoisonKind::Knockout.lifespan())) }),
    ("PoisonSpreadBehaviour.get_Lifespan", |c| { c.with(0)?; Ok(Enc::enc(PoisonKind::Coagulation.lifespan())) }),
    ("PoisonSpreadBehaviour.get_SpreadSpeed", |c| { c.with(0)?; Ok(Enc::enc(PoisonKind::Coagulation.spread_speed())) }),
    ("AcidPoison.get_SpreadSpeed", |c| { c.with(0)?; Ok(Enc::enc(PoisonKind::Acid.spread_speed())) }),
    ("AdrenalinePoison.get_SpreadSpeed", |c| { c.with(0)?; Ok(Enc::enc(PoisonKind::Adrenaline.spread_speed())) }),
    ("DeathPoison.get_SpreadSpeed", |c| { c.with(0)?; Ok(Enc::enc(PoisonKind::Death.spread_speed())) }),
    ("PinkPoison.get_SpreadSpeed", |c| { c.with(0)?; Ok(Enc::enc(PoisonKind::Pink.spread_speed())) }),
    ("SlowZombiePoison.get_SpreadSpeed", |c| { c.with(0)?; Ok(Enc::enc(PoisonKind::SlowZombie.spread_speed())) }),
    ("UltraStrengthPoison.get_SpreadSpeed", |c| { c.with(0)?; Ok(Enc::enc(PoisonKind::UltraStrength.spread_speed())) }),
    ("ZombiePoison.get_SpreadSpeed", |c| { c.with(0)?; Ok(Enc::enc(PoisonKind::Zombie.spread_speed())) }),
    ("UseWireTool.GetWireChannel", |c| { c.with(0)?; Ok(Enc::enc(dll_use_channel("UseWireTool"))) }),
    ("RedUseWireTool.GetWireChannel", |c| { c.with(0)?; Ok(Enc::enc(dll_use_channel("RedUseWireTool"))) }),
    ("BlueUseWireTool.GetWireChannel", |c| { c.with(0)?; Ok(Enc::enc(dll_use_channel("BlueUseWireTool"))) }),
    ("DestructibleRedUseWireTool.GetWireChannel", |c| { c.with(0)?; Ok(Enc::enc(dll_use_channel("DestructibleRedUseWireTool"))) }),
    ("DestructibleBlueUseWireTool.GetWireChannel", |c| { c.with(0)?; Ok(Enc::enc(dll_use_channel("DestructibleBlueUseWireTool"))) }),
    ("UseWireTool.GetWireColor", |c| { c.with(0)?; Ok(Enc::enc(super::super::use_wire::wire_color(dll_use_channel("UseWireTool")))) }),
    ("RedUseWireTool.GetWireColor", |c| { c.with(0)?; Ok(Enc::enc(super::super::use_wire::wire_color(dll_use_channel("RedUseWireTool")))) }),
    ("BlueUseWireTool.GetWireColor", |c| { c.with(0)?; Ok(Enc::enc(super::super::use_wire::wire_color(dll_use_channel("BlueUseWireTool")))) }),
    ("DestructibleRedUseWireTool.GetWireColor", |c| { c.with(0)?; Ok(Enc::enc(super::super::use_wire::wire_color(dll_use_channel("DestructibleRedUseWireTool")))) }),
    ("DestructibleBlueUseWireTool.GetWireColor", |c| { c.with(0)?; Ok(Enc::enc(super::super::use_wire::wire_color(dll_use_channel("DestructibleBlueUseWireTool")))) }),
    ("ChainBehaviour.GetVertexCount", |c| { c.with(0)?; Ok(Enc::enc(WireKind::Chain.vertex_count())) }),
    ("FixedJointWireBehaviour.GetVertexCount", |c| { c.with(0)?; Ok(Enc::enc(WireKind::FixedCable.vertex_count())) }),
    ("HeatPipeBehaviour.GetVertexCount", |c| { c.with(0)?; Ok(Enc::enc(WireKind::HeatPipe.vertex_count())) }),
    ("RigidWireBehaviour.GetVertexCount", |c| { c.with(0)?; Ok(Enc::enc(WireKind::Rigid.vertex_count())) }),
    ("SliderJointWireBehaviour.GetVertexCount", |c| { c.with(0)?; Ok(Enc::enc(WireKind::Slider.vertex_count())) }),
    ("SteelBindingBehaviour.GetVertexCount", |c| { c.with(0)?; Ok(Enc::enc(WireKind::SteelBinding.vertex_count())) }),
    ("WireBehaviour.GetVertexCount", |c| { c.with(0)?; Ok(Enc::enc(WireKind::Energy.vertex_count())) }),
    ("WoodStrutBehaviour.GetVertexCount", |c| { c.with(0)?; Ok(Enc::enc(WireKind::WoodStrut.vertex_count())) }),
    ("CentrifugeBehaviour.get_Limits", |c| { c.with(0)?; Ok(Enc::enc(dll_game().container_limits(Owner::Centrifuge(0)))) }),
    ("LiquidFilterBehaviour.get_Limits", |c| { c.with(0)?; Ok(Enc::enc(dll_game().container_limits(Owner::LiquidFilter(0)))) }),
    ("LiquidPumpBehaviour.get_Limits", |c| { c.with(0)?; Ok(Enc::enc(dll_game().container_limits(Owner::LiquidPump(0)))) }),
    ("ReactorConduitBehaviour.get_Limits", |c| { c.with(0)?; Ok(Enc::enc(dll_game().container_limits(Owner::ReactorConduit(0)))) }),
    ("LiquidCanisterBehaviour.get_AllowsOverflow", |c| { c.with(0)?; Ok(Enc::enc(dll_game().allows_overflow(Owner::LiquidCanister(0)))) }),
    ("LiquidFilterBehaviour.get_AllowsOverflow", |c| { c.with(0)?; Ok(Enc::enc(dll_game().allows_overflow(Owner::LiquidFilter(0)))) }),
    ("LiquidPumpBehaviour.get_AllowsOverflow", |c| { c.with(0)?; Ok(Enc::enc(dll_game().allows_overflow(Owner::LiquidPump(0)))) }),
    ("ReactorConduitBehaviour.get_AllowsOverflow", |c| { c.with(0)?; Ok(Enc::enc(dll_game().allows_overflow(Owner::ReactorConduit(0)))) }),
    ("SyringeBehaviour.get_AllowsOverflow", |c| { c.with(0)?; Ok(Enc::enc(dll_game().allows_overflow(Owner::Syringe(0)))) }),
    ("CirculationBehaviour.get_AllowsTransfer", |c| { c.with(0)?; Ok(Enc::enc(dll_game().wire_allows(Owner::Circulation(0)))) }),
    ("LiquidFilterBehaviour.get_AllowsTransfer", |c| { c.with(0)?; Ok(Enc::enc(dll_game().wire_allows(Owner::LiquidFilter(0)))) }),
    ("LiquidPumpBehaviour.get_AllowsTransfer", |c| { c.with(0)?; Ok(Enc::enc(dll_game().wire_allows(Owner::LiquidPump(0)))) }),
    ("LiquidPumpBehaviour.get_AllowPressureTransfer", |c| { c.with(0)?; Ok(Enc::enc(dll_game().container_allows_pressure(Owner::LiquidPump(0)))) }),
    ("HeatPipeBehaviour.GetHeatTransferFactor", |c| { c.with(0)?; Ok(Enc::enc(super::super::blood_wire::HEAT_PIPE_FACTOR)) }),
    ("DragTool.GetDragFunction", |c| { c.with(0)?; Ok(Enc::enc(super::super::tool::drag_function(false))) }),
    ("GentleDragTool.GetDragFunction", |c| { c.with(0)?; Ok(Enc::enc(super::super::tool::drag_function(true))) }),
    ("GentleDragTool.DragStrengthFunction", |c| { c.with(0)?; Ok(Enc::enc(super::super::tool::GENTLE_DRAG_STRENGTH)) }),
    ("GentleDragTool.GetDraggerMassFunction", |c| { c.with(0)?; Ok(Enc::enc(super::super::tool::GENTLE_DRAGGER_MASS)) }),
    ("CirculationPoison.get_ShouldCallOnEnterEveryUpdate", |c| { c.with(0)?; Ok(Enc::enc(on_enter("CirculationPoison"))) }),
    ("CrushingPoison.get_ShouldCallOnEnterEveryUpdate", |c| { c.with(0)?; Ok(Enc::enc(on_enter("CrushingPoison"))) }),
    ("DurabilitySerum.get_ShouldCallOnEnterEveryUpdate", |c| { c.with(0)?; Ok(Enc::enc(on_enter("DurabilitySerum"))) }),
    ("ExplosionPoison.get_ShouldCallOnEnterEveryUpdate", |c| { c.with(0)?; Ok(Enc::enc(on_enter("ExplosionPoison"))) }),
    ("MassManipulationPoison.get_ShouldCallOnEnterEveryUpdate", |c| { c.with(0)?; Ok(Enc::enc(on_enter("MassManipulationPoison"))) }),
    ("MuscleEnhancementSerum.get_ShouldCallOnEnterEveryUpdate", |c| { c.with(0)?; Ok(Enc::enc(on_enter("MuscleEnhancementSerum"))) }),
    ("MusclePoison.get_ShouldCallOnEnterEveryUpdate", |c| { c.with(0)?; Ok(Enc::enc(on_enter("MusclePoison"))) }),
    ("NumbingPoison.get_ShouldCallOnEnterEveryUpdate", |c| { c.with(0)?; Ok(Enc::enc(on_enter("NumbingPoison"))) }),
    ("OsteomorphosisAgent.get_ShouldCallOnEnterEveryUpdate", |c| { c.with(0)?; Ok(Enc::enc(on_enter("OsteomorphosisAgent"))) }),
    ("ReflectionPoison.get_ShouldCallOnEnterEveryUpdate", |c| { c.with(0)?; Ok(Enc::enc(on_enter("ReflectionPoison"))) }),
    ("RegenerationSerum.get_ShouldCallOnEnterEveryUpdate", |c| { c.with(0)?; Ok(Enc::enc(on_enter("RegenerationSerum"))) }),
    ("SizeManipulationPoison.get_ShouldCallOnEnterEveryUpdate", |c| { c.with(0)?; Ok(Enc::enc(on_enter("SizeManipulationPoison"))) }),
    ("TemporaryBodyLiquid.get_ShouldCallOnEnterEveryUpdate", |c| { c.with(0)?; Ok(Enc::enc(on_enter("TemporaryBodyLiquid"))) }),
    ("TissueDeconstructionAgent.get_ShouldCallOnEnterEveryUpdate", |c| { c.with(0)?; Ok(Enc::enc(on_enter("TissueDeconstructionAgent"))) }),
    ("TransparencyPoison.get_ShouldCallOnEnterEveryUpdate", |c| { c.with(0)?; Ok(Enc::enc(on_enter("TransparencyPoison"))) }),
    ("VestibularPoison.get_ShouldCallOnEnterEveryUpdate", |c| { c.with(0)?; Ok(Enc::enc(on_enter("VestibularPoison"))) }),
    ("ExplosiveAttachmentBehaviour.GetSizeForDamage", |c| { let c = c.with(1)?; let a0 = c.p_f32(0)?; Ok(Enc::enc(super::super::attachment::effect_size_for_damage(a0) as i32)) }),
    ("FollowingViewBehaviour.Transform", |c| { let c = c.with(1)?; let a0 = c.p_string(0)?; Ok(Enc::enc(super::super::Game::following_view_transform(&a0))) }),
    ("PressureGaugeBehaviour.ChargeFunction", |c| { let c = c.with(1)?; let a0 = c.p_f32(0)?; Ok(Enc::enc(super::super::Game::power_gauge_charge_function(a0))) }),
    ("TemperatureGaugeBehaviour.TempFunction", |c| { let c = c.with(1)?; let a0 = c.p_f32(0)?; Ok(Enc::enc(super::super::Game::temperature_gauge_temp_function(a0))) }),
    ("ProceduralShatterMaterialPool.SelectSqrMagnitude", |c| { let c = c.with(1)?; let a0 = c.p_arr::<2>(0)?; Ok(Enc::enc(super::super::shatter::select_sqr_magnitude(a0))) }),
    ("ThermalVisionBehaviour.GetAlphaFor", |c| { let c = c.with(2)?; let a0 = c.p_f32(0)?; let a1 = c.p_f32(1)?; Ok(Enc::enc(super::super::explosion::get_alpha_for(dll_game().AmbientTemperature, a0, a1))) }),
    ("Utils.CelsiusToPreference", |c| { let c = c.with(1)?; let a0 = c.p_f32(0)?; Ok(Enc::enc(super::super::Prefs::default().celsius_to_preference(a0))) }),
    ("TemperatureSettingAttribute.ToPreference", |c| { let c = c.with(1)?; let a0 = c.p_f32(0)?; Ok(Enc::enc(super::super::map::round2(super::super::Game::celsius_to_unit(a0, super::super::Prefs::default().TemperatureUnit)))) }),
    ("TemperatureSettingAttribute.ToCelsius", |c| { let c = c.with(1)?; let a0 = c.p_f32(0)?; Ok(Enc::enc(super::super::map::round2(super::super::Prefs::default().preference_to_celsius(a0)))) }),
    // gapA r20: both overloads; Snap(Vector3, float) is Snap(float, float) per component (its own Approximately(snap, 0) early
    // return is each component's).
    ("Utils.Snap", |c| {
        let c = c.with(2)?;
        let a1 = c.p_f32(1)?;
        if c.c.args[0].contains(',') {
            let v = c.p_arr::<3>(0)?;
            return Ok(v.iter().map(|&x| hex(super::super::mathf::snap(x, a1))).collect::<Vec<_>>().join(","));
        }
        Ok(Enc::enc(super::super::mathf::snap(c.p_f32(0)?, a1)))
    }),
    // gapA r20: the DamagePoint -> Vector4 overload too (its arg is the DamagePoint record).
    ("DamagePoint.op_Implicit", |c| { let c = c.with(1)?;
        if c.c.args[0].starts_with("DamagePoint(") {
            let r = dll_rec(c.c.args[0])?;
            let f = |k: &str| -> Result<f32, String> { dll_bits(dll_get(&r, k)?) };
            let t = dll_get(&r, "Type")?.parse::<i32>().map_err(|_| "Type")?;
            let v = super::super::value_types::DamagePoint::new(f("X")?, f("Y")?, f("Intensity")?, t).to_vector4();
            return Ok(v.iter().map(|&x| hex(x)).collect::<Vec<_>>().join(","));
        }
        let a0 = c.p_arr::<4>(0)?; let p = super::super::value_types::DamagePoint::from_vector4(a0); Ok(format!("DamagePoint(X={} Y={} Intensity={} Type={})", hex(p.X), hex(p.Y), hex(p.Intensity), p.Type)) }),
    ("EmptyModLoader.HasModWithId", |c| { let c = c.with(1)?; let a0 = c.p_string(0)?; Ok(Enc::enc(super::super::mods::LuaModLoader.HasModWithId(&a0))) }),
    ("MapBounds.GetSizeInUnits", |c| { c.with(1)?; Ok(dll_ret(dll_map_bounds(c)?.GetSizeInUnits())) }),
    ("MapBounds.GetCenterInUnits", |c| { c.with(1)?; Ok(dll_ret(dll_map_bounds(c)?.GetCenterInUnits())) }),
    ("MapBounds.GetMinInUnits", |c| { c.with(1)?; Ok(dll_ret(dll_map_bounds(c)?.GetMinInUnits())) }),
    ("MapBounds.GetMaxInUnits", |c| { c.with(1)?; Ok(dll_ret(dll_map_bounds(c)?.GetMaxInUnits())) }),
    ("RotatedRectangle.GetTopLeft", |c| { c.with(1)?; Ok(dll_ret(dll_rect(c)?.get_top_left())) }),
    ("RotatedRectangle.GetTopRight", |c| { c.with(1)?; Ok(dll_ret(dll_rect(c)?.get_top_right())) }),
    ("RotatedRectangle.GetBottomLeft", |c| { c.with(1)?; Ok(dll_ret(dll_rect(c)?.get_bottom_left())) }),
    ("RotatedRectangle.GetBottomRight", |c| { c.with(1)?; Ok(dll_ret(dll_rect(c)?.get_bottom_right())) }),
    ("RotatedRectangle.GetHashCode", |c| { c.with(1)?; Ok(dll_ret(dll_rect(c)?.get_hash_code())) }),
    ("LineSegment.GetDistance", |c| { let w = c.with(2)?; Ok(dll_ret(dll_segment(c)?.get_distance(w.p_arr::<2>(1)?))) }),
    ("LineSegment.GetPointAlong", |c| { let w = c.with(2)?; Ok(dll_ret(dll_segment(c)?.get_point_along(w.p_f32(1)?))) }),
    ("LineSegment.GetHashCode", |c| { c.with(1)?; Ok(dll_ret(dll_segment(c)?.get_hash_code())) }),
    ("DamagePoint.GetHashCode", |c| { c.with(1)?; Ok(dll_ret(super::super::value_types::DamagePoint::new(dll_f32(c, "X")?, dll_f32(c, "Y")?, dll_f32(c, "Intensity")?, dll_i32(c, "Type")?).get_hash_code())) }),
    ("DecalInformation.GetHashCode", |c| { c.with(1)?; Ok(dll_ret(super::super::decal_renderer::DecalInformation { Color: dll_floats(c, "Color")?, Path: dll_string(c, "Path")?, IgnoreRadius: dll_f32(c, "IgnoreRadius")? }.GetHashCode())) }),
    ("Stat.ToString", |c| { c.with(1)?; Ok(dll_ret(super::super::stats::Stat::new(&dll_string(c, "APIName")?).ToString())) }),
    ("Stat.GetHashCode", |c| { c.with(1)?; Ok(dll_ret(super::super::stats::Stat::new(&dll_string(c, "APIName")?).GetHashCode())) }),
    ("FixedIntervalDistributor.get_RateHz", |c| { c.with(1)?; Ok(dll_ret(dll_interval(c)?.get_RateHz())) }),
    ("FixedIntervalDistributor.get_Rate", |c| { c.with(1)?; Ok(dll_ret(dll_interval(c)?.get_RateHz())) }),
    ("FixedIntervalDistributor.set_RateHz", dll_set_rate),
    ("FixedIntervalDistributor.set_Rate", dll_set_rate),
    // gapC r25: the plain auto-properties on DllCheck's seeded objects (no host object needed): the getter's answer is the
    // backing field, the setter's the field it changed and the arg after.
    ("FixedIntervalDistributor.get_MaxCycles", |c| { c.with(1)?; Ok(dll_seeded_row(dll_interval(c)?.MaxCycles.to_string(), &[], &[], &[])) }),
    ("FixedIntervalDistributor.get_MaxRate", |c| { c.with(1)?; Ok(dll_seeded_row(dll_interval(c)?.get_MaxRate().to_string(), &[], &[], &[])) }),
    ("FixedIntervalDistributor.get_InterpolationTime", |c| { c.with(1)?; Ok(dll_seeded_row(hex(dll_interval(c)?.InterpolationTime), &[], &[], &[])) }),
    ("FixedIntervalDistributor.get_IntervalSeconds", |c| { c.with(1)?; Ok(dll_seeded_row(hex(dll_interval(c)?.IntervalSeconds), &[], &[], &[])) }),
    // gapC r26: DecalInstruction.get_size on DllCheck's seeded struct (type is a DecalDescriptor reference DllCheck leaves null).
    ("DecalInstruction.get_size", |c| { c.with(1)?; let d = super::super::decal_renderer::DecalInstruction { globalPosition: dll_floats(c, "<globalPosition>k__BackingField")?, size: dll_f32(c, "<size>k__BackingField")?, colourMultiplier: dll_floats(c, "<colourMultiplier>k__BackingField")?, ..Default::default() }; Ok(dll_seeded_row(hex(d.size), &[], &[], &[])) }),
    // gapC r28: the other DecalInstruction auto-properties on the same seeded struct (gapc28_dll_acc).
    ("DecalInstruction.get_globalPosition", |c| { c.with(1)?; Ok(dll_seeded_row(vec(dll_decal(c)?.globalPosition), &[], &[], &[])) }),
    ("DecalInstruction.get_colourMultiplier", |c| { c.with(1)?; Ok(dll_seeded_row(vec(dll_decal(c)?.colourMultiplier), &[], &[], &[])) }),
    ("DecalInstruction.set_globalPosition", |c| { c.with(2)?; let mut d = dll_decal(c)?; let b = vec(d.globalPosition); let v = dll_vec::<2>(c.args[1])?; d.globalPosition = v; Ok(dll_prop_set("<globalPosition>k__BackingField", b, vec(d.globalPosition), vec(v))) }),
    ("DecalInstruction.set_size", |c| { c.with(2)?; let mut d = dll_decal(c)?; let b = hex(d.size); let v = dll_bits(c.args[1])?; d.size = v; Ok(dll_prop_set("<size>k__BackingField", b, hex(d.size), hex(v))) }),
    ("DecalInstruction.set_colourMultiplier", |c| { c.with(2)?; let mut d = dll_decal(c)?; let b = vec(d.colourMultiplier); let v = dll_vec::<4>(c.args[1])?; d.colourMultiplier = v; Ok(dll_prop_set("<colourMultiplier>k__BackingField", b, vec(d.colourMultiplier), vec(v))) }),
    ("FixedIntervalDistributor.set_MaxCycles", |c| { let w = c.with(2)?; let mut f = dll_interval(c)?; let b = f.MaxCycles.to_string(); let v: i32 = w.p_int(1)?; f.MaxCycles = v; Ok(dll_prop_set("<MaxCycles>k__BackingField", b, f.MaxCycles.to_string(), v.to_string())) }),
    ("FixedIntervalDistributor.set_MaxRate", |c| { let w = c.with(2)?; let mut f = dll_interval(c)?; let b = f.MaxCycles.to_string(); let v: i32 = w.p_int(1)?; f.set_MaxRate(v); Ok(dll_prop_set("<MaxCycles>k__BackingField", b, f.MaxCycles.to_string(), v.to_string())) }),
    ("FixedIntervalDistributor.set_InterpolationTime", |c| { let w = c.with(2)?; let mut f = dll_interval(c)?; let b = hex(f.InterpolationTime); let v = w.p_f32(1)?; f.InterpolationTime = v; Ok(dll_prop_set("<InterpolationTime>k__BackingField", b, hex(f.InterpolationTime), hex(v))) }),
    ("FixedIntervalDistributor.set_IntervalSeconds", |c| { let w = c.with(2)?; let mut f = dll_interval(c)?; let b = hex(f.IntervalSeconds); let v = w.p_f32(1)?; f.IntervalSeconds = v; Ok(dll_prop_set("<IntervalSeconds>k__BackingField", b, hex(f.IntervalSeconds), hex(v))) }),
    ("ActivationPropagation.get_Traversed", |c| { c.with(1)?; let mut s = super::super::signal::Signal::default(); s.TraversedCount = dll_i32(c, "TraversedCount")?; Ok(dll_seeded_row(s.get_Traversed().to_string(), &[], &[], &[])) }),
    ("ActivationPropagation.set_Traversed", |c| {
        let w = c.with(2)?;
        let mut s = super::super::signal::Signal::default();
        s.TraversedCount = dll_i32(c, "TraversedCount")?;
        let b = s.TraversedCount.to_string();
        let v: i32 = w.p_int(1)?;
        s.set_Traversed(v);
        Ok(dll_prop_set("TraversedCount", b, s.TraversedCount.to_string(), v.to_string()))
    }),
    ("ThermalVisionBehaviour.CelsiusToData", |c| { let w = c.with(2)?; Ok(dll_ret(super::super::explosion::ThermalTexture::new(dll_f32(c, "MinTemp")?, dll_f32(c, "MaxTemp")?, 0, [0.0; 2], [0.0; 2]).celsius_to_data(w.p_f32(1)?))) }),
    // Diffuse: the DLL's static PhysicalBehaviour.AmbientTemperature is unset under DllCheck (0).
    ("ReactorCoreBehaviour.Diffuse", |c| {
        let id = dll_self_field(c, "phys")?;
        let id = id.rsplit('#').next().unwrap_or_default().to_string();
        let t = u32::from_str_radix(&dll_link_field(c, "phys", "Temperature")?, 16).map(f32::from_bits).map_err(|e| e.to_string())?;
        let fdt = dll_f32(c, "@fdt")?;
        dll_reactor(c, 1, |g, _| {
            g.AmbientTemperature = 0.0;
            g.time.fixedDeltaTime = fdt;
            g.physicals.push(super::super::Physical::default());
            g.physicals[0].Temperature = t;
            g.reactor_cores[0].physical = 0;
            g.reactor_diffuse(0);
            let after = g.physicals[0].Temperature;
            Ok(if after.to_bits() != t.to_bits() { format!("void @{id}.Temperature={}", hex(after)) } else { "void".into() })
        })
        .map(|s| match s.split_once(" @") { Some((v, rest)) => { let (link, set) = rest.split_once(" set(").unwrap(); format!("{v} set(@{link} {set}").replace(" )", ")") } None => s })
    }),
    ("CarBehaviour.SetHealth", |c| {
        let w = c.with(2)?;
        let mut g = dll_game();
        let mut s = super::super::car::CarBehaviour::default();
        dll_load!(c, s);
        s.id = 1;
        let before = dll_show!(s);
        g.cars.push(s);
        g.car_invoke(1, super::super::car::CarCall::SetHealth(w.p_f32(1)?));
        Ok(format!("void {}", dll_changes(&before, &dll_show!(g.cars[0]))))
    }),
    ("CarBehaviour.OnEMPHit", |c| {
        let mut g = dll_game();
        let mut s = super::super::car::CarBehaviour::default();
        dll_load!(c, s);
        let before = dll_show!(s);
        g.cars.push(s);
        g.car_on_emp_hit(0);
        Ok(format!("void {}", dll_changes(&before, &dll_show!(g.cars[0]))))
    }),
    // LagboxBehaviour.Awake only looks up the MuteBehaviour; on a TimedGateBehaviour its override also zeroes the
    // PhysicalBehaviour's ActivationPropagationDelay (timed_gate_awake).
    ("LagboxBehaviour.Awake", |c| {
        if !c.args.first().is_some_and(|s| s.starts_with("TimedGateBehaviour(")) {
            return Ok("void set()".into());
        }
        let (id, rec) = dll_link_of_type(c, "PhysicalBehaviour")?;
        let before = f32::from_bits(u32::from_str_radix(&dll_record_field(&rec, "ActivationPropagationDelay")?, 16).map_err(|e| e.to_string())?);
        let mut g = dll_game();
        g.physicals.push(super::super::Physical::default());
        g.physicals[0].ActivationPropagationDelay = before;
        g.timed_gates.push(super::super::timed_gate::TimedGate::default());
        g.timed_gate_awake(0);
        let after = g.physicals[0].ActivationPropagationDelay;
        Ok(format!("void set({})", if after.to_bits() != before.to_bits() { format!("@{id}.ActivationPropagationDelay={}", hex(after)) } else { String::new() }))
    }),
    // GetAveragePosition with nothing followed (the recorded CurrentlyFollowing is empty): targetPos.
    ("CameraControlBehaviour.GetAveragePosition", |c| {
        if dll_self_field(c, "CurrentlyFollowing")? != "[]" {
            return Err("CurrentlyFollowing not empty".into());
        }
        let mut g = dll_game();
        g.camera_control.targetPos = dll_floats(c, "targetPos")?;
        Ok(format!("{} set()", Enc::enc(g.camera_average_position())))
    }),
    // Awake: Container's min and max (Bounds: center -/+ extents, as link.rs loads them), FancyEffects (the DLL's
    // UserPreferenceManager.Current default: on), the 16 particles' random positions and particlePositions.
    ("LiquidContainerController.Awake", |c| {
        let bounds = dll_self_field(c, "Container")?;
        let fl = |name: &str| -> Result<[f32; 3], String> {
            let v = dll_record_field(&bounds, name)?;
            let mut a = [0f32; 3];
            for (i, x) in v.split(',').enumerate().take(3) { a[i] = f32::from_bits(u32::from_str_radix(x, 16).map_err(|e| e.to_string())?); }
            Ok(a)
        };
        let (center, extents) = (fl("m_Center")?, fl("m_Extents")?);
        let mut g = dll_game();
        g.random = dll_random(c)?;
        let mut s = super::super::flask::LiquidContainer::default();
        s.min = [center[0] - extents[0], center[1] - extents[1]];
        s.max = [center[0] + extents[0], center[1] + extents[1]];
        g.liquid_containers.push(s);
        g.liquid_container_awake(0);
        let s = &g.liquid_containers[0];
        let v4 = |p: &[f32; 4]| p.iter().map(|x| hex(*x)).collect::<Vec<_>>().join(",");
        let after = [
            ("isFancy", if s.fancy { "1".to_string() } else { "0".into() }),
            ("particlePositions", s.particlePositions.as_ref().map_or("null".into(), |v| format!("[{}]", v.iter().map(v4).collect::<Vec<_>>().join(";")))),
            ("particles", format!("[{}]", s.particles.iter().map(|p| format!("Particle(Position={},{} Velocity={},{})", hex(p.Position[0]), hex(p.Position[1]), hex(p.Velocity[0]), hex(p.Velocity[1]))).collect::<Vec<_>>().join(";"))),
        ];
        let before: Vec<(&str, String)> = after.iter().map(|(n, _)| (*n, dll_self_field(c, n).unwrap_or_default())).collect();
        Ok(format!("void {}", dll_changes(&before, &after)))
    }),
    // set_Color: the color field (the property block's _LiquidColor is the engine's).
    ("LiquidContainerController.set_Color", |c| {
        let w = c.with(2)?;
        let mut g = dll_game();
        let mut s = super::super::flask::LiquidContainer::default();
        let before: [f32; 4] = dll_floats(c, "color")?;
        s.color = before;
        g.liquid_containers.push(s);
        g.liquid_container_set_color(0, w.p_arr::<4>(1)?);
        let after = g.liquid_containers[0].color;
        Ok(format!("void set({})", if after.map(f32::to_bits) != before.map(f32::to_bits) { format!("color={}", Enc::enc(after)) } else { String::new() }))
    }),
    // ExistByChance.Start: a row is a call that kept its object (SetActive(false) is the engine's, a thrown case).
    ("ExistByChance.Start", |c| {
        let mut g = dll_game();
        g.random = dll_random(c)?;
        if g.exist_by_chance_loses(dll_f32(c, "Chance")?) {
            return Ok("void set(SetActive(false))".into());
        }
        Ok("void set()".into())
    }),
    // CreateHole(localPos, localDir, intensity): the rows are the calls under the 0.1 intensity gate (the hole is an
    // Instantiate, the engine's: a thrown case).
    ("LiquidContainerLeakingBehaviour.CreateHole", |c| {
        let w = c.with(4)?;
        let mut g = dll_game();
        g.liquid_leaks.push(super::super::liquid_leak::LiquidLeak::default());
        let before = dll_show!(g.liquid_leaks[0]);
        g.liquid_leak_create_hole(0, w.p_arr::<2>(1)?, w.p_arr::<2>(2)?, w.p_f32(3)?);
        Ok(format!("void {}", dll_changes(&before, &dll_show!(g.liquid_leaks[0]))))
    }),
    ("BellClip.GetDisplayName", |c| {
        let w = c.with(2)?;
        // (A newline as `\n`, as DllCheck writes it: a row is one line.)
        let s = super::super::universal_bell::bell_display_name(&dll_string(c, "Name")?, dll_i32(c, "Key")?, w.p_bool(1)?);
        Ok(format!("\"{}\" set()", s.replace(' ', "_").replace('\n', "\\n")))
    }),
    // GridToWorldPoint(x, y): the cell times GridSize.
    ("AmbientTemperatureGridBehaviour.GridToWorldPoint", |c| {
        let w = c.with(3)?;
        let mut g = dll_game();
        g.ambient_grid.GridSize = dll_i32(c, "GridSize")?;
        Ok(dll_ret(g.GridToWorldPoint(w.p_int(1)?, w.p_int(2)?)))
    }),
    ("DeltaInt.Increment", |c| { let w = c.with(2)?; dll_delta_int(c, |d| Ok(d.increment(w.p_int(1)?))) }),
    ("DeltaInt.Decrement", |c| { c.with(1)?; dll_delta_int(c, |d| Ok(d.decrement())) }),
    ("DeltaInt.UpdatePreviousValue", |c| { c.with(1)?; dll_delta_int(c, |d| Ok(d.update_previous_value())) }),
    // get_LargeExplosionParticles: EffectSize as link.rs decodes it (Medium, Small, Large).
    ("ExplosionParameters.get_LargeExplosionParticles", |c| {
        c.with(1)?;
        use super::super::explosion::{EffectSize, ExplosionParameters};
        let EffectSize = match dll_i32(c, "EffectSize")? { 0 => EffectSize::Medium, 1 => EffectSize::Small, 2 => EffectSize::Large, e => return Err(format!("EffectSize {e}")) };
        Ok(dll_ret(ExplosionParameters { EffectSize, ..Default::default() }.large_explosion_particles()))
    }),
    ("FanBehaviour.IsFan", |c| {
        c.with(1)?;
        let mut g = dll_game();
        g.tr.fan_behaviour.push(super::super::fan::FanBehaviour { Mode: dll_i32(c, "Mode")?, ..Default::default() });
        Ok(dll_ret(g.fan_behaviour_is_fan(0)))
    }),
    // UpdateNodes: from the last, a node at or below 0 life goes, the others lose deltaTime (@dt).
    ("EnergySwordMotionBlur.UpdateNodes", |c| {
        c.with(1)?;
        let before = dll_self_field(c, "nodes")?;
        let mut g = dll_game();
        g.time.deltaTime = dll_f32(c, "@dt")?;
        let mut s = super::super::energy_sword::EnergySwordBlur::default();
        for n in dll_items(&before)? {
            let r = dll_rec(n)?;
            let v3 = |k: &str| -> Result<[f32; 3], String> {
                let v: Vec<f32> = dll_get(&r, k)?.split(',').map(dll_bits).collect::<Result<_, _>>()?;
                v.try_into().map_err(|_| format!("{k} not a Vector3"))
            };
            s.nodes.push(super::super::energy_sword::BlurNode { Life: dll_bits(dll_get(&r, "Life")?)?, A: v3("A")?, B: v3("B")? });
        }
        g.energy_blurs.push(s);
        g.energy_blur_update_nodes(0);
        let v3 = |v: [f32; 3]| v.iter().map(|x| hex(*x)).collect::<Vec<_>>().join(",");
        let after = dll_list(g.energy_blurs[0].nodes.iter().map(|n| format!("Node(Life={} A={} B={})", hex(n.Life), v3(n.A), v3(n.B))));
        Ok(format!("void {}", dll_changes(&[("nodes", before)], &[("nodes", after)])))
    }),
    ("DetectorBehaviour.get_IsFlipped", |c| {
        c.with(1)?;
        Ok(dll_ret(super::super::detector::detector_is_flipped(dll_floats::<3>(c, "@lscale")?)))
    }),
    ("ContraptionSavePoint.SetName", |c| {
        let w = c.with(2)?;
        let mut s = super::super::contraption::ContraptionSavePoint::default();
        s.NameToSave = dll_string(c, "NameToSave")?;
        s.lockName = dll_i32(c, "lockName")? != 0;
        let before = Enc::enc(s.NameToSave.clone());
        s.SetName(&w.p_string(1)?);
        let after = Enc::enc(s.NameToSave.clone());
        Ok(format!("void set({})", if after != before { format!("NameToSave={after}") } else { String::new() }))
    }),
    // FixedUpdate: the linked PhysicalBehaviour's Temperature lerps to TargetTemperature while active, the machinery
    // whole and the temperature on the side ActiveCooling works from.
    ("HeatingElementBehaviour.FixedUpdate", |c| {
        c.with(1)?;
        let mut g = dll_game();
        let mut s = super::super::devices::HeatingElement::default();
        s.HeatTargetSpeed = dll_f32(c, "HeatTargetSpeed")?;
        s.ActiveCooling = dll_i32(c, "ActiveCooling")? != 0;
        s.TargetTemperature = dll_f32(c, "TargetTemperature")?;
        s.Activated = dll_i32(c, "Activated")? != 0;
        g.heating_elements.push(s);
        g.physicals.push(super::super::Physical::default());
        let before = dll_bits(&dll_link_field(c, "PhysicalBehaviour", "Temperature")?)?;
        g.physicals[0].Temperature = before;
        g.machinery.push(Default::default());
        g.machinery[0].Destroyed = dll_link_field(c, "DamagableMachineryBehaviour", "Destroyed")? == "1";
        g.heating_element_fixed_update(0);
        let after = g.physicals[0].Temperature;
        let id = dll_self_field(c, "PhysicalBehaviour")?.rsplit('#').next().unwrap_or_default().to_string();
        Ok(format!("void set({})", if after.to_bits() != before.to_bits() { format!("@{id}.Temperature={}", hex(after)) } else { String::new() }))
    }),
    ("GuideTemperatureBehaviour.FixedUpdate", |c| {
        c.with(1)?;
        let mut g = dll_game();
        let mut s = super::super::small_machines::GuideTemperatureBehaviour::default();
        s.TargetTemperature = dll_f32(c, "TargetTemperature")?;
        s.GuideSpeed = dll_f32(c, "GuideSpeed")?;
        s.phys = Some(0);
        g.tr.guide_temperature_behaviour.push(s);
        g.physicals.push(super::super::Physical::default());
        let before = dll_bits(&dll_link_field(c, "phys", "Temperature")?)?;
        g.physicals[0].Temperature = before;
        g.guide_temperature_behaviour_fixed_update(0);
        let after = g.physicals[0].Temperature;
        let id = dll_self_field(c, "phys")?.rsplit('#').next().unwrap_or_default().to_string();
        Ok(format!("void set({})", if after.to_bits() != before.to_bits() { format!("@{id}.Temperature={}", hex(after)) } else { String::new() }))
    }),
    // GetDisplayText: the action (by its recorded Name, spaces as `_`) bound to the recorded Key and SecondaryKey.
    ("InputAction.GetDisplayText", |c| {
        c.with(1)?;
        let defs = &*crate::ui::tables::ACTIONS;
        let name = dll_string(c, "Name")?;
        let i = defs.iter().position(|a| a.name.replace(' ', "_") == name).ok_or_else(|| format!("no action {name}"))?;
        let mut g = dll_game();
        let mut binds: Vec<(String, String)> = defs.iter().map(|a| (a.key.to_string(), a.secondary.to_string())).collect();
        binds[i] = (super::super::menu::key_name(dll_i32(c, "Key")?), super::super::menu::key_name(dll_i32(c, "SecondaryKey")?));
        g.control_binds = Some(binds);
        Ok(dll_ret(g.input_display_text(defs[i].id)))
    }),
    ("BoatMotorBehaviour.Reverse", |c| {
        c.with(1)?;
        let mut g = dll_game();
        let mut s = super::super::motors::BoatMotorBehaviour::default();
        s.Force = dll_f32(c, "Force")?;
        s.VisualRotationSpeed = dll_f32(c, "VisualRotationSpeed")?;
        let before = [("Force", hex(s.Force)), ("VisualRotationSpeed", hex(s.VisualRotationSpeed))];
        g.tr.boat_motor_behaviour.push(s);
        g.boat_motor_behaviour_reverse(0);
        let s = &g.tr.boat_motor_behaviour[0];
        Ok(format!("void {}", dll_changes(&before, &[("Force", hex(s.Force)), ("VisualRotationSpeed", hex(s.VisualRotationSpeed))])))
    }),
    // FixedUpdate: a limb below a tenth of its health draws Random.value; above 0.9995 a spark is Instantiated (the
    // engine's: a thrown case), so a row is a call that spawned none.
    ("AndroidSparkCreator.FixedUpdate", |c| {
        c.with(1)?;
        let mut g = dll_game();
        // (No @rng: the call drew no Random, so the default one stands.)
        if let Ok(r) = dll_random(c) { g.random = r; }
        g.limbs.push(super::super::limb::Limb::default());
        g.limbs[0].Health = dll_bits(&dll_link_field(c, "limb", "Health")?)?;
        g.limbs[0].InitialHealth = dll_bits(&dll_link_field(c, "limb", "InitialHealth")?)?;
        g.android_sparks.push(Default::default());
        let spawned = dll_quiet(|| g.android_spark_fixed_update(0)).is_none();
        Ok(if spawned { "Instantiate(spark)".into() } else { "void set()".into() })
    }),
    // Update: the Blades' ShouldRotate (Activated, the machinery whole) and
    // RotationSpeedTarget (9000 * (Charge + 1)).
    ("FanBehaviour.Update", |c| {
        c.with(1)?;
        // (A HelicopterAudioSource's volumes are presentation: the twin checks the Blades.)
        let mut g = dll_game();
        g.tr.fan_behaviour.push(super::super::fan::FanBehaviour { Activated: dll_i32(c, "Activated")? != 0, Blades: Some(0), DamagableMachinery: Some(0), physicalBehaviour: Some(0), ..Default::default() });
        let mut p = super::super::Physical::default();
        p.charge = dll_bits(&dll_link_field(c, "physicalBehaviour", "charge")?)?;
        p.ForceNoCharge = dll_link_field(c, "physicalBehaviour", "ForceNoCharge")? == "1";
        g.physicals.push(p);
        g.machinery.push(Default::default());
        g.machinery[0].Destroyed = dll_link_field(c, "DamagableMachinery", "Destroyed")? == "1";
        let mut b = super::super::fan::CosmeticRotationBehaviour::default();
        b.ShouldRotate = dll_link_field(c, "Blades", "ShouldRotate")? == "1";
        b.RotationSpeedTarget = dll_bits(&dll_link_field(c, "Blades", "RotationSpeedTarget")?)?;
        let show = |b: &super::super::fan::CosmeticRotationBehaviour| [("RotationSpeedTarget", hex(b.RotationSpeedTarget)), ("ShouldRotate", (b.ShouldRotate as i32).to_string())];
        let before = show(&b);
        g.tr.cosmetic_rotation_behaviour.push(b);
        g.fan_behaviour_update(0);
        let after = show(&g.tr.cosmetic_rotation_behaviour[0]);
        let id = dll_self_field(c, "Blades")?.rsplit('#').next().unwrap_or_default().to_string();
        let changed: Vec<String> = after.iter().zip(&before).filter(|(a, b)| a.1 != b.1).map(|(a, _)| format!("@{id}.{}={}", a.0, a.1)).collect();
        Ok(format!("void set({})", changed.join(" ")))
    }),
    ("LiquidContainerController.GetClipThreshold", |c| { c.with(1)?; Ok(dll_ret(dll_liquid_container(c)?.liquid_clip_threshold(0))) }),
    ("LiquidContainerController.get_OffsetRepulsionDistance", |c| { c.with(1)?; Ok(dll_ret(dll_liquid_container(c)?.liquid_offset_repulsion_distance(0))) }),
    // Equals(DamagePoint): PartialEq (the float fields with ==).
    ("DamagePoint.Equals", |c| {
        c.with(2)?;
        let point = |s: &str| -> Result<super::super::value_types::DamagePoint, String> {
            let r = dll_rec(s)?;
            let f = |k: &str| dll_get(&r, k).and_then(dll_bits);
            Ok(super::super::value_types::DamagePoint::new(f("X")?, f("Y")?, f("Intensity")?, dll_get(&r, "Type")?.parse().map_err(|_| "Type".to_string())?))
        };
        if !c.args[1].starts_with("DamagePoint(") {
            return Err(format!("the object overload with {}", c.args[1]));
        }
        Ok(dll_ret(point(c.args[0])? == point(c.args[1])?))
    }),
    ("LiquidPumpBehaviour.get_Pressure", |c| {
        c.with(1)?;
        let mut g = dll_game();
        g.liquid_pumps.push(super::super::liquid_pump::LiquidPump { Mode: dll_i32(c, "Mode")?, ..Default::default() });
        Ok(dll_ret(g.container_pressure(super::super::container::Owner::LiquidPump(0))))
    }),
    ("BloodTankBehaviour.get_Pressure", |c| {
        c.with(1)?;
        let mut g = dll_game();
        g.blood_tanks.push(super::super::blood_tank::BloodTank { Mode: dll_i32(c, "Mode")?, ..Default::default() });
        Ok(dll_ret(g.container_pressure(super::super::container::Owner::BloodTank(0))))
    }),
    ("LiquidCanisterBehaviour.get_Limits", |c| {
        c.with(1)?;
        let mut g = dll_game();
        g.liquid_canisters.push(super::super::liquid_canister::LiquidCanister { Capacity: dll_f32(c, "Capacity")?, ..Default::default() });
        Ok(dll_ret(g.container_limits(super::super::container::Owner::LiquidCanister(0))))
    }),
    ("LimbBehaviour.OnEMPHit", |c| {
        let mut g = dll_game();
        g.limbs.push(super::super::limb::Limb::default());
        g.limbs[0].IsAndroid = dll_self_field(c, "IsAndroid")? == "1";
        g.limbs[0].Health = dll_f32(c, "Health")?;
        let before = g.limbs[0].Health;
        g.limb_on_emp_hit(0);
        let h = g.limbs[0].Health;
        Ok(if h.to_bits() == before.to_bits() { "void set()".into() } else { format!("void set(Health={})", h.enc()) })
    }),
    // Stabbed(stabbing): recorded rows (destroyed machines, null stabbers) and seeded ones (intact machines, Indestructible,
    // stabbers with and without StabCausesWound, from their records: the self's `@link#id`). A stab that breaks it
    // (SetAsBroken, its own check) is not among the rows.
    ("DamagableMachineryBehaviour.Stabbed", |c| {
        let mut g = dll_game();
        g.machinery.push(super::super::machinery::Machinery::default());
        g.physicals.push(super::super::Physical::default());
        g.machinery[0].Indestructible = dll_self_field(c, "Indestructible")? == "1";
        g.machinery[0].Destroyed = dll_self_field(c, "Destroyed")? == "1";
        g.machinery[0].Health = dll_f32(c, "Health")?;
        g.machinery[0].StabDamageMultiplier = dll_f32(c, "StabDamageMultiplier")?;
        let stabber = c.args[1].split("stabber=").nth(1).and_then(|s| s.split(' ').next()).ok_or("no stabber")?;
        g.physicals[0].StabCausesWound = match stabber.rsplit_once('#') {
            Some((_, id)) => dll_record_field(&dll_self_field(c, &format!("@link#{id}"))?, "StabCausesWound")? == "1",
            None if g.machinery[0].Indestructible || g.machinery[0].Destroyed => true,
            None => return Err("reads the null stabber".into()),
        };
        let before = dll_show!(g.machinery[0]);
        let m = &g.machinery[0];
        if !m.Indestructible && !m.Destroyed && g.physicals[0].StabCausesWound && !(m.Health - m.StabDamageMultiplier > 0.0) { return Err("breaks".into()); }
        g.machinery_stabbed(0, 0);
        Ok(format!("void {}", dll_changes(&before, &dll_show!(g.machinery[0]))))
    }),
    // FixedUpdate: the rows are the steps up to 30 (past them it reads the person, a linked object).
    ("GorseDroolingBehaviour.FixedUpdate", |c| {
        let mut g = dll_game();
        let step = dll_i32(c, "step")?;
        if step >= 30 { return Err("past step 30".into()); }
        g.gorse_droolings.push(super::super::gorse::GorseDrooling {
            id: 0, limb: 0, anchor: Default::default(), chain: vec![], CanSee: 0, Projectile: None, FireClips: 0, Range: 0.0, Force: 0.0,
            step, isAlive: dll_self_field(c, "isAlive")? == "1", emission_set: false,
        });
        g.gorse_drooling_fixed_update(0);
        Ok(format!("void set(step={})", g.gorse_droolings[0].step))
    }),
    ("BulbBehaviour.OnEMPHit", |c| {
        let mut g = dll_game();
        g.bulbs.push(Default::default());
        g.bulbs[0].FilamentHealth = dll_f32(c, "FilamentHealth")?;
        let before = dll_show!(g.bulbs[0]);
        g.bulb_on_emp_hit(0);
        Ok(format!("void {}", dll_changes(&before, &dll_show!(g.bulbs[0]))))
    }),
    ("LimbBehaviour.GetOriginalBloodType", |c| {
        let mut g = dll_game();
        g.limbs.push(super::super::limb::Limb::default());
        g.limbs[0].BloodLiquidType = dll_string(c, "BloodLiquidType")?;
        Ok(format!("{} set()", super::super::circulation::liquid_identity(g.blood_type(0)).replace(' ', "_")))
    }),
    ("GoreStringBehaviour.Awake", |c| {
        let mut s = super::super::limb::GoreString::default();
        s.TissueColour = dll_floats::<4>(c, "TissueColour")?;
        s.smoothedColour = dll_floats::<4>(c, "smoothedColour")?;
        let before = s.smoothedColour;
        s.awake();
        Ok(if before.map(f32::to_bits) == s.smoothedColour.map(f32::to_bits) { "void set()".into() } else { format!("void set(smoothedColour={})", s.smoothedColour.enc()) })
    }),
    ("DestroyWhenAllChildrenDestroyed.Check", |c| {
        let mut g = dll_game();
        let l = dll_i32(c, "l")?;
        g.dwacds.push(super::super::destroyable::DestroyWhenAllChildrenDestroyed { id: 0, l });
        g.dwacd_check(0);
        let after = g.dwacds[0].l;
        Ok(format!("void set({})", if after != l { format!("l={after}") } else { String::new() }))
    }),
    ("DisableWhenControlTrigger.get_Identity", |c| {
        let t = super::super::disable_trigger::DisableWhenControlTrigger {
            id: 0,
            name: dll_self_field(c, "@go")?,
            Control: dll_string(c, "Control")?,
            IdentityOverride: dll_string(c, "IdentityOverride")?,
            NextStep: None,
            NeverSkip: false,
        };
        Ok(format!("{} set()", Enc::enc(t.identity())))
    }),
    ("HologramJitter.Awake", |c| {
        let mut g = dll_game();
        g.random = dll_random(c)?;
        let before = dll_f32(c, "offset")?;
        let after = g.hologram_jitter_offset();
        Ok(format!("void set({})", if after.to_bits() != before.to_bits() { format!("offset={}", hex(after)) } else { String::new() }))
    }),
    // SetColor writes the FancyBloodSplatController link's DecalColourMultiplier (and the particles' start colour, the engine's).
    ("BloodImpactBehaviour.SetColor", |c| {
        let w = c.with(2)?;
        let link = dll_self_field(c, "FancyBloodSplatController")?;
        let id = link.rsplit('#').next().unwrap_or_default().to_string();
        let before: [f32; 4] = {
            let v = dll_link_field(c, "FancyBloodSplatController", "DecalColourMultiplier")?;
            let mut a = [0f32; 4];
            for (i, x) in v.split(',').enumerate().take(4) { a[i] = f32::from_bits(u32::from_str_radix(x, 16).map_err(|e| e.to_string())?); }
            a
        };
        let mut g = dll_game();
        let mut s = super::super::misc::FancyBloodSplat::default();
        s.DecalColourMultiplier = before;
        g.splats.push(s);
        g.blood_puffs.push(super::super::misc::BloodPuff { active: true, splat: 0, emitter_active: true, off: Vec::new() });
        g.blood_impact_set_color(0, w.p_arr::<4>(1)?);
        let after = g.splats[0].DecalColourMultiplier;
        Ok(format!("void set({})", if after.map(f32::to_bits) != before.map(f32::to_bits) { format!("@{id}.DecalColourMultiplier={}", Enc::enc(after)) } else { String::new() }))
    }),
    ("GateBehaviour.SetThreshold", |c| {
        let w = c.with(2)?;
        let mut g = dll_game();
        let mut s = super::super::timed_gate::Gate::default();
        dll_load!(c, s);
        // gapA r20: base.enabled is the row's @enabled; PlayClipOnce(Tick) runs on the linked PhysicalBehaviour's audioSource.
        s.enabled = dll_i32(c, "@enabled")? == 1;
        s.id = 1;
        s.physical = 0;
        let (source, sid) = dll_physical_audio(&mut g, &dll2_link(c, "PhysicalBehaviour")?)?;
        dll_clip_field(&mut g, 1, "Tick", &dll_self_field(c, "Tick")?);
        let before = dll_show!(s);
        g.gates.push(s);
        g.gate_set_threshold(0, w.p_f32(1)?);
        let set = dll_changes(&before, &dll_show!(g.gates[0]));
        Ok(format!("void {}", dll_with_fx(&set, &dll_source_fx(&g, sid, &source))))
    }),
    ("ReactorCoreBehaviour.CalculateCriticality", |c| dll_reactor(c, 1, |g, _| { g.reactor_calculate_criticality(0); Ok("void".into()) })),
    ("ReactorCoreBehaviour.get_PowerOutput", |c| dll_reactor(c, 1, |g, _| Ok(hex(g.reactor_cores[0].powerOutput)))),
    ("ReactorCoreBehaviour.IncreaseCooling", |c| dll_reactor_call(c, super::super::reactor::ReactorCall::IncreaseCooling)),
    ("ReactorCoreBehaviour.DecreaseCooling", |c| dll_reactor_call(c, super::super::reactor::ReactorCall::DecreaseCooling)),
    ("ReactorCoreBehaviour.IncreasePower", |c| dll_reactor_call(c, super::super::reactor::ReactorCall::IncreasePower)),
    ("ReactorCoreBehaviour.DecreasePower", |c| dll_reactor_call(c, super::super::reactor::ReactorCall::DecreasePower)),
    ("ReactorCoreBehaviour.ForceMeltdown", |c| dll_reactor_call(c, super::super::reactor::ReactorCall::ForceMeltdown)),
    ("ReactorCoreBehaviour.DecrementCoolingPower", |c| dll_reactor(c, 2, |g, w| { g.reactor_core_call(1, super::super::reactor::ReactorCall::DecrementCoolingPower(w.p_f32(1)?)); Ok("void".into()) })),
    ("MotorisedHingeBehaviour.GetScaledDelta", |c| {
        c.with(1)?;
        let mut g = dll_on_physical(c)?;
        let mut s = super::super::motors::MotorisedHingeBehaviour::default();
        dll_load!(c, s);
        s.this_physical = Some(0);
        g.tr.motorised_hinge_behaviour.push(s);
        Ok(dll_ret(g.motorised_hinge_behaviour_get_scaled_delta(0)))
    }),
    ("RotorBehaviour.GetScaledSpeed", |c| {
        c.with(1)?;
        let mut g = dll_on_physical(c)?;
        let mut s = super::super::motors::RotorBehaviour::default();
        dll_load!(c, s);
        s.this_physical = Some(0);
        g.tr.rotor_behaviour.push(s);
        Ok(dll_ret(g.rotor_behaviour_get_scaled_speed(0)))
    }),
    ("VibrationBehaviour.get_ForceVector", |c| {
        c.with(1)?;
        let mut g = dll_on_physical(c)?;
        g.time.time = dll_f32(c, "@time")?;
        let mut s = super::super::centrifuge::Vibration::default();
        dll_load!(c, s);
        g.vibrations.push(s);
        Ok(dll_ret(g.vibration_force_vector(0)))
    }),
    ("AntennaBehaviour.GetSignalStrength", |c| {
        c.with(1)?;
        let g = dll_on_physical(c)?;
        let x = super::super::blood_wire::Antenna { a: 0, antenna: 0, CorrectUp: dll_floats(c, "CorrectUp")?, Fuzziness: dll_f32(c, "Fuzziness")?, receiver: 0 };
        Ok(dll_ret(g.antenna_signal_strength(x)))
    }),
    ("MotorisedHingeBehaviour.SetDelta", |c| {
        let w = c.with(2)?;
        let mut g = dll_on_physical(c)?;
        let mut s = super::super::motors::MotorisedHingeBehaviour::default();
        dll_load!(c, s);
        s.this_physical = Some(0);
        let before = dll_show!(s);
        g.tr.motorised_hinge_behaviour.push(s);
        g.motorised_hinge_behaviour_set_delta(0, w.p_f32(1)?);
        Ok(format!("void {}", dll_changes(&before, &dll_show!(g.tr.motorised_hinge_behaviour[0]))))
    }),
    ("LineSegment.GetRandomPoint", |c| { c.with(1)?; Ok(dll_ret(dll_segment(c)?.get_random_point(&mut dll_random(c)?))) }),
];
