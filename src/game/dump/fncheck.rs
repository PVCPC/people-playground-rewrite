//! --fn-check <probe recording>: per-function checks against the real game (the recorder's PPG_FNPROBE, tools/recorder).
//! The recorder sets seeded inputs on the spawned human, takes a full snapshot "PI" with an FnCase record (the function,
//! its target's instance id, the args), calls the game's own method through reflection and takes a snapshot "PO" with an
//! FnResult record (the return value, the args after the call). Here each PI is loaded into a fresh Game, the
//! rewrite's function is called with the same target and args, and the return value and the whole PO state (the
//! snapshot comparison of the loop tests) must match bit for bit. Methods that read no object state come as
//! `fnpure <name> args=<..> ret=<..>` lines. Output: one `PASS <name> <cases>` / `FAIL ...` line per function.
use super::super::container::Owner;
use super::super::Game;
use super::{compare, f, floats, header, reference, snapshots, Snapshot};
use crate::game::Field;
use crate::game_assets::HumanRig;
use std::path::Path;

/// A coverage build (RUSTFLAGS="-C instrument-coverage --cfg fncov", tools/recorder/fncov_via.sh): the counters of each
/// case's call alone, summed over a function's passing cases, written as `$PPG_FNCOV/<name>.profraw` for the functions
/// listed in `$PPG_FNCOV_FNS`. An inlined game method is credited through the enclosing fn only when those ran its lines.
#[cfg(fncov)]
pub(crate) mod cov {
    use std::collections::{BTreeMap, HashSet};
    use std::sync::Mutex;
    unsafe extern "C" {
        fn __llvm_profile_begin_counters() -> *mut u64;
        fn __llvm_profile_end_counters() -> *mut u64;
        fn __llvm_profile_reset_counters();
        fn __llvm_profile_set_filename(name: *const std::os::raw::c_char);
        fn __llvm_profile_write_file() -> i32;
    }
    static LAST: Mutex<Vec<(u32, u64)>> = Mutex::new(Vec::new());
    static SUMS: Mutex<BTreeMap<String, BTreeMap<u32, u64>>> = Mutex::new(BTreeMap::new());
    fn counters() -> &'static mut [u64] {
        unsafe {
            let (b, e) = (__llvm_profile_begin_counters(), __llvm_profile_end_counters());
            std::slice::from_raw_parts_mut(b, e.offset_from(b) as usize)
        }
    }
    fn wanted() -> &'static HashSet<String> {
        static W: std::sync::OnceLock<HashSet<String>> = std::sync::OnceLock::new();
        W.get_or_init(|| std::env::var("PPG_FNCOV_FNS").ok().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default().split_whitespace().map(String::from).collect())
    }
    pub fn start() {
        unsafe { __llvm_profile_reset_counters() }
    }
    pub fn stash() {
        *LAST.lock().unwrap() = counters().iter().enumerate().filter(|(_, c)| **c != 0).map(|(i, c)| (i as u32, *c)).collect();
    }
    pub fn passed(name: &str) {
        if !wanted().contains(name) { return }
        let mut sums = SUMS.lock().unwrap();
        let s = sums.entry(name.to_string()).or_default();
        for &(i, c) in LAST.lock().unwrap().iter() {
            *s.entry(i).or_default() += c;
        }
    }
    pub fn write() {
        let Ok(dir) = std::env::var("PPG_FNCOV") else { return };
        for (name, s) in SUMS.lock().unwrap().iter() {
            let c = counters();
            c.fill(0);
            for (&i, &v) in s { c[i as usize] = v }
            let f = std::ffi::CString::new(format!("{dir}/{name}.profraw")).unwrap();
            unsafe {
                __llvm_profile_set_filename(f.as_ptr());
                __llvm_profile_write_file();
            }
        }
        // The exit handler's own write goes nowhere useful.
        let f = std::ffi::CString::new(format!("{dir}/exit.profraw.tmp")).unwrap();
        unsafe { __llvm_profile_set_filename(f.as_ptr()) }
    }
}

/// One case: the PI snapshot, the target's instance id and the recorded args.
pub(super) struct Case<'a> {
    s: &'a Snapshot,
    pub(super) id: i32,
    /// The component the method ran on (`self=`): the target, or another component on its GameObject.
    sid: i32,
    pub(super) args: Vec<&'a str>,
    /// A Liquid's method (the recorder's `liquid=`): self is that registry liquid, its LIQUIDS index.
    liq: Option<usize>,
    /// The recorder's version (the header's `recver N`; 0 before gapB r2 stamped one).
    ver: u32,
}

/// gapB r2: the recorder version this checkout's recorder stamps (tools/recorder/Recorder.cs RecVer). A field a newer
/// recorder added is asked for with `Case::field_since`: an older recording's case is "too old", not a failure.
pub(super) const REC_VER: u32 = 4;

impl Case<'_> {
    /// gapC: a kind the game fixes by the self component's class (PowerKind::of_tool, WireKind::of_tool): `of` the self
    /// record's class (as pin_wood reads WoodPinTool/PinTool).
    /// gapC r2: the FnCase's `selfty=` (FnTiny.ToolPre: a tool the recorder added has no record) first.
    pub(super) fn self_kind<T>(&self, of: fn(&str) -> Option<T>) -> Result<T, String> {
        let ty = match self.field("selfty") {
            Some(t) => t,
            None => self.s.records.iter().find(|r| r.id == self.sid && r.ty != "FnCase").ok_or_else(|| format!("no self record #{}", self.sid))?.ty.as_str(),
        };
        of(ty).ok_or_else(|| format!("no kind for the self class {ty}"))
    }
    fn index(&self, ty: &str) -> Result<usize, String> {
        self.s.of(ty).position(|r| r.id == self.id).ok_or_else(|| format!("no {ty} #{}", self.id))
    }
    fn limb(&self) -> Result<usize, String> {
        self.index("LimbBehaviour")
    }
    /// gapA r16: the record id of the `ty` component on the same GameObject as component `id` (its Clone row's comps).
    fn sibling_id(&self, id: i32, ty: &str) -> Result<Option<i32>, String> {
        let key = format!(":{id}");
        let comps = self.s.of("Clone").filter_map(|r| r.get("comps")).find(|x| x.trim_matches(['[', ']']).split(';').any(|e| e.ends_with(&key))).ok_or_else(|| format!("no Clone row with #{id}"))?;
        Ok(comps.trim_matches(['[', ']']).split(';').find_map(|e| e.strip_prefix(ty)?.strip_prefix(':')?.parse().ok()))
    }
    /// gapA r16: a GoreStringBehaviour's limb: the target is the limb, or (Start) the GoreString itself beside it.
    fn gore_limb(&self) -> Result<usize, String> {
        self.limb().or_else(|e| {
            let lid = self.sibling_id(self.sid, "LimbBehaviour")?.ok_or(e)?;
            self.s.of("LimbBehaviour").position(|r| r.id == lid).ok_or_else(|| format!("no LimbBehaviour #{lid}"))
        })
    }
    /// CirculationBehaviour records list in LimbBehaviour order (from_snapshot checks it).
    fn circ(&self) -> Result<usize, String> {
        self.index("CirculationBehaviour")
    }
    fn phys(&self, g: &Game) -> Result<usize, String> {
        phys_by_id(g, self.id)
    }
    fn f(&self, i: usize) -> f32 {
        f(self.args[i])
    }
    fn v<const N: usize>(&self, i: usize) -> [f32; N] {
        floats(self.args[i])
    }
    /// Arg `i` as the recorder formats it (Dump.Format: float bits, 0/1, an integer, a vector's float bits).
    pub(super) fn arg<T: Field + Default>(&self, i: usize) -> Result<T, String> {
        let mut v = T::default();
        let a = self.args.get(i).ok_or(format!("no arg {i}"))?;
        // A Component (`@name#id`) passed where the rewrite takes its instance id (a Collider2D).
        let id = a.starts_with('@').then(|| reference(a).map_or("0".into(), |id| id.to_string()));
        v.parse(id.as_deref().unwrap_or(a))?;
        Ok(v)
    }
    /// A field of a Shot argument (`Shot(normal=x,y point=x,y damage=f ...)`): the rewrite takes the fields it reads.
    /// An older recording passed the fields themselves: then arg `i`.
    pub(super) fn shot<T: Field + Default>(&self, key: &str, i: usize) -> Result<T, String> {
        if !self.args.iter().any(|a| a.starts_with("Shot(")) { return self.arg(i) }
        self.rec("Shot(", key)
    }
    /// A field of the first `<prefix>k=v k=v)` arg (a Shot, MapConfigProbe's Stab).
    pub(super) fn rec<T: Field + Default>(&self, prefix: &str, key: &str) -> Result<T, String> {
        let a = self.args.iter().find(|a| a.starts_with(prefix)).ok_or_else(|| format!("no {prefix} arg"))?;
        let v = a.trim_start_matches(prefix).trim_end_matches(')').split(' ').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('=')).ok_or_else(|| format!("no {key} in {a}"))?;
        let mut t = T::default();
        t.parse(v)?;
        Ok(t)
    }
    fn b(&self, i: usize) -> bool {
        self.args[i] == "1"
    }
    /// Arg i, a string (Fmt writes it quoted).
    fn arg_str(&self, i: usize) -> Result<&str, String> {
        let a = self.args.get(i).ok_or(format!("no arg {i}"))?;
        a.strip_prefix('"').and_then(|a| a.strip_suffix('"')).ok_or_else(|| format!("arg {i} is not a string: {a}"))
    }
    fn n<T: std::str::FromStr>(&self, i: usize) -> Result<T, String> {
        self.args[i].parse().map_err(|_| format!("arg {i}: {}", self.args[i]))
    }
    /// A field of the case's FnCase record (BigProbeB's `em=`: a BallisticsEmitter's state).
    pub(super) fn field(&self, k: &str) -> Option<&str> {
        self.s.of("FnCase").next()?.get(k)
    }
    /// A FnCase field the recorder writes since version `since`: missing from an older recording is "too old: ..." (fn_check
    /// counts such a case apart, neither pass nor fail).
    pub(super) fn field_since(&self, k: &str, since: u32) -> Result<&str, String> {
        self.field(k).ok_or_else(|| if self.ver < since { format!("too old: recorder {} has no {k}= (since {since})", self.ver) } else { format!("no {k}= in the FnCase") })
    }
    /// The `self=` component's record index among its type's records (a limb's SkinMaterialHandler: the limb index).
    fn comp(&self, ty: &str) -> Result<usize, String> {
        self.s.of(ty).position(|r| r.id == self.sid).ok_or_else(|| format!("no {ty} #{}", self.sid))
    }
    /// Arg i, a component reference (`@name#id`): its record index among `ty`'s records.
    fn arg_index(&self, i: usize, ty: &str) -> Result<usize, String> {
        let id = reference(self.args[i]).ok_or_else(|| format!("arg {i} is no reference"))?;
        self.s.of(ty).position(|r| r.id == id).ok_or_else(|| format!("no {ty} #{id}"))
    }
    /// fnmaped: a Penetration argument (`Penetration(Stabber=@.. Joint=@.. ..)`), the arena entry with its Stabber and Joint.
    fn penetration(&self, g: &Game, i: usize) -> Result<usize, String> {
        super::fnmaped::penetration_arg(g, self.args.get(i).ok_or("no Penetration argument")?)
    }
    /// fnmaped: a Joint2D argument as its `fixed_joints` index; None for a joint the loader adopts none for (a limb's hinge).
    fn adopted_joint(&self, g: &Game, i: usize) -> Option<usize> {
        reference(self.args.get(i)?).and_then(|id| g.adopted_joints.get(&id).copied())
    }
    /// fnmaped: the SerialisedLayerBehaviour self's object: its record's `phys=` PhysicalBehaviour.
    fn layer_self(&self, g: &Game) -> Result<usize, String> {
        let r = self.s.of("SerialisedLayer").find(|r| r.id == self.sid).ok_or_else(|| format!("no SerialisedLayer #{}", self.sid))?;
        phys_by_id(g, r.req("phys")?.parse().map_err(|_| "SerialisedLayer phys")?)
    }
    fn arg_phys(&self, g: &Game, i: usize) -> Result<usize, String> {
        phys_by_id(g, reference(self.args[i]).ok_or_else(|| format!("arg {i} is no reference"))?)
    }
    fn liquid(&self, i: usize) -> Result<usize, String> {
        let id = self.args[i].replace('_', " ");
        super::super::circulation::LIQUIDS.iter().position(|(n, _)| *n == id).ok_or_else(|| format!("no liquid {id}"))
    }
    /// A Liquid method's self (the registry liquid of the case's class).
    fn liq(&self) -> Result<usize, String> {
        self.liq.ok_or_else(|| "no liquid self".to_string())
    }
    fn circ_owner(&self) -> Result<Owner, String> {
        Ok(Owner::Circulation(self.circ()?))
    }
}

/// A generated call's view of a case (fncheck_gen.rs): rewrite parameter j is the self index when the game method is an
/// instance one and the rewrite function takes it (one more parameter than the recorded args), else arg j - off.
struct With<'a, 'b> {
    c: &'a Case<'b>,
    off: usize,
}

impl Case<'_> {
    fn with(&self, n: usize) -> Result<With<'_, '_>, String> {
        let off = if (self.sid != 0 || self.liq.is_some()) && n == self.args.len() + 1 { 1 } else { 0 };
        if n != self.args.len() + off {
            return Err(format!("arity: the rewrite takes {n}, the game passed {} args", self.args.len()));
        }
        Ok(With { c: self, off })
    }
    /// A method taking one Shot that the rewrite takes as the self index and the Shot's fields it reads, by name
    /// (fncheck_gen's `c.c.shot("damage", 0)`).
    fn with_shot(&self, _n: usize) -> Result<With<'_, '_>, String> {
        if self.sid == 0 || self.args.len() != 1 || !self.args[0].starts_with("Shot(") {
            return Err(format!("not one Shot arg: {:?}", self.args));
        }
        Ok(With { c: self, off: 1 })
    }
    /// Rewrite param 0 is the self index; the game's arguments past the rewrite's are not passed on (OnFlip's scale,
    /// which the rewrite reads from state).
    fn with_self(&self, n: usize) -> Result<With<'_, '_>, String> {
        if self.sid == 0 || self.args.len() + 1 < n {
            return Err(format!("arity: the rewrite takes {n} with self, the game passed {} args", self.args.len()));
        }
        Ok(With { c: self, off: 1 })
    }
    /// The loaded map's name (MapLoaderBehaviour.CurrentMap): the map Clone record's `map`.
    fn map_name(&self) -> Result<String, String> {
        Ok(self.s.of("Clone").find(|r| r.get("root") == Some("Map")).ok_or("no map")?.req("map")?.replace('_', " "))
    }
    /// A record id's index: a PhysicalBehaviour's in the physicals arena, any other's among its type's records.
    fn record_index(&self, g: &Game, id: i32) -> Result<(String, usize), String> {
        let Some(r) = self.s.records.iter().find(|r| r.id == id && r.ty != "FnCase") else {
            // A person's root component with no record of its own (HingeJointLimitAutofixBehaviour): the person's index,
            // from the PersonBehaviour beside it on the root's Clone record.
            let key = format!(":{id}");
            let person = self.s.of("Clone").filter_map(|c| c.get("comps")).find(|comps| comps.trim_matches(|x| x == '[' || x == ']').split(';').any(|e| e.ends_with(&key)));
            let pid = person.and_then(|comps| comps.trim_matches(|x| x == '[' || x == ']').split(';').find_map(|e| e.strip_prefix("PersonBehaviour:")).and_then(|p| p.parse::<i32>().ok()));
            return match pid {
                Some(pid) if pid != id => self.record_index(g, pid),
                // The map's weather lightning (MapConfig's clone, no record of its own): its arena index.
                // gapB r8: a map scene script with no record (Abyss's EggBehaviour, Reactor A5's LoopSoundBehaviour): the arena the map's link fills.
                _ => g.lightnings.iter().position(|l| l.id == id).map(|i| ("WeatherLightningBehaviour".into(), i))
                    .or_else(|| g.eggs.iter().position(|x| x.id == id).map(|i| ("EggBehaviour".into(), i)))
                    .or_else(|| g.loop_sounds.iter().position(|x| x.id == id).map(|i| ("LoopSoundBehaviour".into(), i)))
                    // fc41 r20: Reactor A5's JointMotorSoundBehaviours.
                    .or_else(|| g.joint_motor_sounds.iter().position(|x| x.id == id).map(|i| ("JointMotorSoundBehaviour".into(), i)))
                    .ok_or_else(|| format!("no record #{id}")),
            };
        };
        if r.ty == "PhysicalBehaviour" {
            return Ok((r.ty.clone(), phys_by_id(g, id)?));
        }
        // A Rigidbody2D (Utils' extensions): the PhysicalBehaviour whose `rigidbody` it is.
        if r.ty == "Rigidbody2D" {
            let key = format!("#{id}");
            let pb = self.s.of("PhysicalBehaviour").find(|x| x.get("rigidbody").is_some_and(|v| v.ends_with(&key))).ok_or_else(|| format!("no PhysicalBehaviour with Rigidbody2D #{id}"))?;
            return Ok(("PhysicalBehaviour".into(), phys_by_id(g, pb.id)?));
        }
        // fnweap: a wire script: its `Game::blood_wires` index.
        if super::fnweap::is_wire_record(r) {
            return Ok((r.ty.clone(), super::fnweap::wire_index(g, id).ok_or(format!("no wire #{id}"))?));
        }
        // Map records (Dump.MapTypes) of an arena the map's link fills: the index of its component id.
        if r.ty == "ReactorA5DisplayBehaviour" {
            return Ok((r.ty.clone(), g.reactor_a5_displays.iter().position(|d| d.id == id).ok_or(format!("no display #{id}"))?));
        }
        if r.ty == "OffsetGridBehaviour" {
            return Ok((r.ty.clone(), g.offset_grids.iter().position(|d| d.id == id).ok_or(format!("no offset grid #{id}"))?));
        }
        // fnweap: a pin: its `Game::pins` index.
        if r.ty == "PinBehaviour" {
            return Ok((r.ty.clone(), g.pins.iter().position(|p| p.id == id).ok_or(format!("no pin #{id}"))?));
        }
        // fnweap: a belt: its `Game::belts` index.
        if r.ty == "SerpentineBeltBehaviour" {
            return Ok((r.ty.clone(), super::fnweap::belt_index(g, id).ok_or(format!("no belt #{id}"))?));
        }
        // fnrec r2: a power tool: its `Game::powers` index (by kind; the load adds each recorded one).
        if let Some(k) = crate::game::power::PowerKind::of_tool(&r.ty) {
            return Ok((r.ty.clone(), g.powers.iter().position(|p| p.kind == k).ok_or(format!("no power #{id}"))?));
        }
        // An item's script (ItemScript records of every class): its index among its class's, the rewrite's arena order.
        if r.ty == "ItemScript" {
            let class = r.get("class");
            return Ok((r.ty.clone(), self.s.of("ItemScript").filter(|x| x.get("class") == class).position(|x| x.id == id).unwrap()));
        }
        Ok((r.ty.clone(), self.s.of(&r.ty).position(|x| x.id == id).unwrap()))
    }
}

impl With<'_, '_> {
    /// The component the method ran on (an item's: with_component_instance finds its clone).
    fn sid(&self) -> i32 { self.c.sid }
    fn arg(&self, j: usize) -> Result<&str, String> {
        if j < self.off { return Err(format!("param {j} is the self index")) }
        Ok(self.c.args[j - self.off])
    }
    fn p_f32(&self, j: usize) -> Result<f32, String> {
        let a = self.arg(j)?;
        // A Shot passed where the rewrite takes one float: its damage (the field every such Shot reads).
        if a.starts_with("Shot(") {
            return self.c.shot("damage", j - self.off);
        }
        u32::from_str_radix(a, 16).map(f32::from_bits).map_err(|_| format!("arg {j} not a float: {a}"))
    }
    fn p_f64(&self, j: usize) -> Result<f64, String> {
        let a = self.arg(j)?;
        u64::from_str_radix(a, 16).map(f64::from_bits).map_err(|_| format!("arg {j} not a double: {a}"))
    }
    fn p_bool(&self, j: usize) -> Result<bool, String> {
        match self.arg(j)? { "1" => Ok(true), "0" => Ok(false), a => Err(format!("arg {j} not a bool: {a}")) }
    }
    /// Param 0 as an integer when it is the self: the game's instance id (a component arena's `id`).
    fn p_int<T: std::str::FromStr>(&self, j: usize) -> Result<T, String> {
        if j < self.off {
            return str::parse::<T>(&self.c.sid.to_string()).map_err(|_| format!("param {j}: self id {} not this integer type", self.c.sid));
        }
        let a = self.arg(j)?;
        if a.starts_with('@') {
            return str::parse(&reference(a).ok_or("null reference")?.to_string()).map_err(|_| format!("arg {j}: id of {a} not this integer type"));
        }
        a.parse().map_err(|_| format!("arg {j} not an integer: {a}"))
    }
    /// A Vector2/3/4 or Color (its floats; a Vector3 to a [f32; 2] drops z).
    fn p_arr<const N: usize>(&self, j: usize) -> Result<[f32; N], String> {
        let a = self.arg(j)?;
        let mut v = [0.0; N];
        for (i, w) in a.split(',').take(N).enumerate() {
            v[i] = u32::from_str_radix(w, 16).map(f32::from_bits).map_err(|_| format!("arg {j} not floats: {a}"))?;
        }
        Ok(v)
    }
    fn p_vec2(&self, j: usize) -> Result<crate::b2::math::Vec2, String> {
        let [x, y] = self.p_arr::<2>(j)?;
        Ok(crate::b2::math::Vec2::new(x, y))
    }
    fn p_string(&self, j: usize) -> Result<String, String> {
        Ok(self.arg(j)?.trim_matches('"').to_string())
    }
    /// The self index, a component reference's index, an integer, or a liquid's index.
    fn p_usize(&self, g: &Game, j: usize) -> Result<usize, String> {
        if j < self.off {
            if let Some(l) = self.c.liq {
                return Ok(l);
            }
            return Ok(self.c.record_index(g, self.c.sid)?.1);
        }
        let a = self.arg(j)?;
        if a.starts_with('@') {
            return Ok(self.c.record_index(g, reference(a).ok_or("null reference")?)?.1);
        }
        if let Ok(n) = a.parse() { return Ok(n) }
        let id = a.replace('_', " ");
        super::super::circulation::LIQUIDS.iter().position(|(n, _)| *n == id).ok_or_else(|| format!("arg {j}: {a}"))
    }
    /// A Game::visuals index (its classes share the arena): the entry with the component's id.
    fn p_visual(&self, g: &Game, j: usize) -> Result<usize, String> {
        let id = if j < self.off { self.c.sid } else { reference(self.arg(j)?).ok_or("null reference")? };
        g.visuals.iter().position(|v| v.id == id).ok_or_else(|| {
            // The nearest ids the arena has (an id offset shows a runtime object the rewrite made, or skipped, before it).
            let mut near: Vec<i32> = g.visuals.iter().map(|v| v.id).collect();
            near.sort_by_key(|&v| (v - id).abs());
            format!("no visual #{id} (nearest {:?})", &near[..near.len().min(6)])
        })
    }
    fn p_opt_usize(&self, g: &Game, j: usize) -> Result<Option<usize>, String> {
        if j >= self.off && matches!(self.arg(j)?, "null" | "destroyed") { return Ok(None) }
        self.p_usize(g, j).map(Some)
    }
    /// A container: a limb's circulation by its record, any other by its arena's component id.
    fn p_owner(&self, g: &Game, j: usize) -> Result<Owner, String> {
        let id = if j < self.off { self.c.sid } else { reference(self.arg(j)?).ok_or("null reference")? };
        let (ty, i) = self.c.record_index(g, id)?;
        if ty == "CirculationBehaviour" { return Ok(Owner::Circulation(i)) }
        macro_rules! by_id { ($($arena:ident => $v:ident),*) => { $(if let Some(i) = g.$arena.iter().position(|x| x.id == id) { return Ok(Owner::$v(i)) })* } }
        by_id!(flasks => Flask, syringes => Syringe, valves => Valve, liquid_duplicators => LiquidDuplicator, liquid_pumps => LiquidPump,
            centrifuges => Centrifuge, liquidentifiers => Liquidentifier, liquid_canisters => LiquidCanister, liquid_filters => LiquidFilter,
            blood_tanks => BloodTank, liquid_outlets => LiquidOutlet, reactor_conduits => ReactorConduit);
        Err(format!("no container Owner #{id}"))
    }
    /// A list arg (Dump.Format's `[a;b;...]`), each item through `f`.
    fn p_list<T>(&self, j: usize, f: impl Fn(&str) -> Result<T, String>) -> Result<Vec<T>, String> {
        let a = self.arg(j)?;
        let a = a.strip_prefix('[').and_then(|a| a.strip_suffix(']')).ok_or_else(|| format!("arg {j} not a list: {a}"))?;
        split_args(a).into_iter().map(f).collect()
    }
    /// A Collision2D (the recorder's `Collision2D(collider=id other=id rigidbody=id otherRigidbody=id rv=x,y enabled=b
    /// contacts=[point,normal,separation,normalImpulse,tangentImpulse,relativeVelocity,enabled,collider,other,rigidbody,otherRigidbody;..])`,
    /// ids the components' instance ids, 0 for null).
    fn p_collision(&self, j: usize) -> Result<crate::b2::contacts2d::ScriptCollision, String> {
        use crate::b2::{contacts2d::ScriptCollision, math::Vec2};
        let a = self.arg(j)?;
        let body = a.strip_prefix("Collision2D(").and_then(|a| a.strip_suffix(')')).ok_or_else(|| format!("arg {j} not a Collision2D: {a}"))?;
        let kv: std::collections::HashMap<&str, &str> = super::split_top(body, ' ').into_iter().filter_map(|w| w.split_once('=')).collect();
        let get = |k: &str| kv.get(k).copied().ok_or_else(|| format!("Collision2D without {k}"));
        let id = |s: &str| s.parse::<i32>().map_err(|_| format!("Collision2D id {s}"));
        let opt = |s: &str| id(s).map(|i| (i != 0).then_some(i));
        let v2 = |x: &str, y: &str| Vec2::new(f(x), f(y));
        let rv: Vec<&str> = get("rv")?.split(',').collect();
        let contacts = get("contacts")?.trim_start_matches('[').trim_end_matches(']');
        let contacts = split_args(contacts).into_iter().map(super::fnmaped::contact_point).collect::<Result<Vec<_>, String>>()?;
        Ok(ScriptCollision {
            collider: id(get("collider")?)?, other_collider: id(get("other")?)?, rigidbody: opt(get("rigidbody")?)?, other_rigidbody: opt(get("otherRigidbody")?)?,
            relative_velocity: v2(rv[0], rv[1]), enabled: get("enabled")? == "1", contacts,
        })
    }
    /// A list item: a component reference's index (as p_usize) or an integer.
    fn item_usize(&self, g: &Game, s: &str) -> Result<usize, String> {
        if s.starts_with('@') { return Ok(self.c.record_index(g, reference(s).ok_or("null reference")?)?.1) }
        s.parse().map_err(|_| format!("item not an index: {s}"))
    }
    /// A null arg (a nullable, a null string): None.
    fn p_opt<T>(&self, j: usize, f: impl Fn(&Self, usize) -> Result<T, String>) -> Result<Option<T>, String> {
        if j >= self.off && self.arg(j)? == "null" { return Ok(None) }
        f(self, j).map(Some)
    }
    /// A Vector2Int (`Vector2Int(m_X=1 m_Y=2)`) or `x,y`: its last two integers.
    fn p_pair(&self, j: usize) -> Result<(i32, i32), String> {
        let a = self.arg(j)?;
        let n: Vec<i32> = a.split(|ch: char| !(ch.is_ascii_digit() || ch == '-')).filter_map(|w| w.parse().ok()).collect();
        match n[..] { [.., x, y] => Ok((x, y)), _ => Err(format!("arg {j} not a pair: {a}")) }
    }
}

/// A list item as an integer: a reference's instance id, or the number.
fn item_int<T: std::str::FromStr>(s: &str) -> Result<T, String> {
    let n = reference(s).map(|id| id.to_string()).unwrap_or_else(|| s.to_string());
    str::parse::<T>(&n).map_err(|_| format!("item not an integer: {s}"))
}
fn item_f32(s: &str) -> Result<f32, String> {
    u32::from_str_radix(s, 16).map(f32::from_bits).map_err(|_| format!("item not a float: {s}"))
}
fn item_bool(s: &str) -> Result<bool, String> {
    match s { "1" => Ok(true), "0" => Ok(false), _ => Err(format!("item not a bool: {s}")) }
}
fn item_arr<const N: usize>(s: &str) -> Result<[f32; N], String> {
    let mut v = [0.0; N];
    for (i, w) in s.split(',').take(N).enumerate() { v[i] = item_f32(w)? }
    Ok(v)
}

/// The recorder's `;`-joined args (or list items), a list's or a struct's own `;`s kept inside it.
fn split_args(s: &str) -> Vec<&str> {
    let (mut out, mut depth, mut at) = (Vec::new(), 0i32, 0);
    for (i, ch) in s.char_indices() {
        match ch {
            '[' | '{' | '(' => depth += 1,
            ']' | '}' | ')' => depth -= 1,
            ';' if depth == 0 => { out.push(&s[at..i]); at = i + 1 }
            _ => {}
        }
    }
    out.push(&s[at..]);
    out.retain(|a| !a.is_empty());
    out
}

/// A rewrite return value as the recorder's Dump.Format writes the game's.
/// A method taking the game's AssetDb: Game::assets (opened on first use), taken out for the call.
fn with_assets<R>(g: &mut Game, f: impl FnOnce(&mut Game, &mut crate::assets::db::AssetDb) -> R) -> R {
    let mut db = g.assets.take().unwrap_or_else(|| {
        let game = crate::game_assets::find_game(None).expect("the game's data");
        Box::new(crate::assets::db::AssetDb::open(&game).expect("the game's data"))
    });
    let r = f(g, &mut db);
    g.assets = Some(db);
    r
}

/// An IEnumerator method's result as the recorder writes it: its first MoveNext (StartCoroutine runs it at once) and
/// what that yielded.
fn routine_step(g: &mut Game, r: &mut crate::game::coroutine::Routine) -> String {
    use crate::game::coroutine::Yield;
    match r(g) {
        Yield::Null => "step:null".into(),
        Yield::WaitForSeconds(s) => format!("step:WaitForSeconds({})", hex(s)),
        Yield::WaitForSecondsRealtime(s) => format!("step:WaitForSecondsRealtime({})", hex(s)),
        Yield::WaitForFixedUpdate => "step:WaitForFixedUpdate".into(),
        Yield::WaitForEndOfFrame => "step:WaitForEndOfFrame".into(),
        Yield::WaitUntil(_) => "step:WaitUntil".into(),
        Yield::Routine(_) => "step:Routine".into(),
        Yield::Done => "step:end".into(),
    }
}

trait Enc {
    fn enc(self) -> String;
}
/// The game's return in the rewrite's terms: each `@name#id` object its record's arena index, and a two-field
/// Vector2Int (m_X, m_Y) or ValueTuple (Item1, Item2) struct `a,b`.
/// Arg 0 of a UseWireTool case, a wire's DistanceJoint2D: its `Game::fixed_joints` entry.
fn use_wire_joint(g: &Game, c: &Case) -> Result<usize, String> {
    let id = reference(c.args[0]).ok_or("null joint")?;
    super::fnweap::wire_joint(g, c.s, id).ok_or(format!("joint #{id}: no wire's"))
}

/// A BallisticsEmitter.CallbackParams argument (`CallbackParams(HitObject=.. SurfaceNormal=.. Direction=.. Position=..)`): Position, Direction.
fn callback_params(a: &str) -> Result<([f32; 2], [f32; 2]), String> {
    if !a.starts_with("CallbackParams(") {
        return Err(format!("not a CallbackParams: {a}"));
    }
    let fields = super::inner_fields(a);
    let get = |k: &str| fields.iter().find(|(n, _)| n == k).map(|x| floats::<2>(&x.1)).ok_or(format!("CallbackParams without {k}"));
    Ok((get("Position")?, get("Direction")?))
}

/// gapB r1: field `k` of a struct argument as Dump.Format writes it (`Name(k=v k=v)`).
fn arg_field<'a>(a: &'a str, k: &str) -> Option<&'a str> {
    let inner = a.split_once('(')?.1.strip_suffix(')')?;
    super::split_top(inner, ' ').into_iter().find_map(|kv| kv.strip_prefix(k)?.strip_prefix('='))
}

/// fc41: PersonBehaviour.SetShaderColour and its callers: every limb's four material colours the rewrite keeps (FnTiny.StateOf
/// " stpre=pc:" / " st=pc:", per limb in Limbs order `r,g,b,a|..` in _BruiseColor, _SecondBruiseColor, _ThirdBruiseColor, _Zombie
/// order) loaded into Skin colours before the call and given back after it.
fn person_colour(g: &mut Game, c: &Case, call: impl FnOnce(&mut Game, usize) -> Result<(), String>) -> Result<String, String> {
    let p = c.index("PersonBehaviour")?;
    let limbs = g.people[p].Limbs.clone();
    let (pre, rt) = st_pre(c).and_then(|s| s.strip_prefix("pc:")).ok_or("no pc: stpre")?.split_once("#rt:").ok_or("no #rt: table")?;
    // Unity's Material.GetColor in a linear-colour-space project: SetColor(b / 255) reads back as rt[b] (FnTiny.GammaRoundTrip).
    let rt: Vec<f32> = rt.split(',').map(|x| u32::from_str_radix(x, 16).map(f32::from_bits).map_err(|_| format!("bad rt {x}"))).collect::<Result<_, _>>()?;
    if rt.len() != 256 { return Err(format!("#rt: has {} values", rt.len())) }
    let pre: Vec<&str> = pre.split(';').collect();
    if pre.len() != limbs.len() { return Err(format!("pc: stpre has {} limbs, ours {}", pre.len(), limbs.len())) }
    let mut before = Vec::new();
    // fc41 r10: SetBloodColour's cases list five colours (_BloodColor last), the others four: compare as many as listed.
    let n = pre.first().map_or(0, |s| s.split('|').count());
    for (&l, s) in limbs.iter().zip(&pre) {
        for (slot, v) in s.split('|').enumerate() {
            let v: [f32; 4] = floats(v);
            *g.skins[l].colours.get_mut(slot).ok_or("pc: more than five colours")? = Some(v);
        }
        before.push(g.skins[l].colours);
    }
    call(g, p)?;
    // A colour the call set is stored as given and read back through the round trip; an untouched one is what was loaded.
    let read_back = |x: f32| -> f32 { let b = (x * 255.0).round(); if (0.0..=255.0).contains(&b) && b / 255.0 == x { rt[b as usize] } else { x } };
    let hex = |v: Option<[f32; 4]>| v.map_or("null".into(), |v| v.map(|x| format!("{:08x}", x.to_bits())).join(","));
    let out: Vec<String> = limbs.iter().zip(&before).map(|(&l, was)| g.skins[l].colours.iter().zip(was).take(n).map(|(v, w)| hex(if v != w { v.map(|[r, gr, bl, a]| [read_back(r), read_back(gr), read_back(bl), a]) } else { *v })).collect::<Vec<_>>().join("|")).collect();
    Ok(format!(" st=pc:{}", out.join(";")))
}

/// gapC r28: the FnCase's " stpre=" (FnTiny.StatePre: the private state after the seed, before the call).
fn st_pre<'a>(c: &Case<'a>) -> Option<&'a str> {
    c.s.of("FnCase").next().and_then(|r| r.get("stpre"))
}

fn game_ret_plain(g: &Game, s: &Snapshot, ret: &str) -> String {
    for (head, x, y) in [("Vector2Int(", "m_X=", " m_Y="), ("ValueTuple`2(", "Item1=", " Item2=")] {
        if let Some((a, b)) = ret.strip_prefix(head).and_then(|r| r.strip_suffix(')')).and_then(|r| r.strip_prefix(x)).and_then(|r| r.split_once(y)) {
            return format!("{a},{b}");
        }
    }
    let case = Case { s, id: 0, sid: 0, args: Vec::new(), liq: None, ver: REC_VER };
    let mut out = String::new();
    let mut rest = ret;
    while let Some(at) = rest.find('@') {
        out.push_str(&rest[..at]);
        // `@name#id`: the id's digits end it (a struct's `hover=@X#-1 SelectionChanged=null)` goes on after it).
        let end = rest[at..].find('#').map_or(rest.len(), |h| {
            let digits = &rest[at + h + 1..];
            at + h + 1 + digits.find(|ch: char| !(ch == '-' || ch.is_ascii_digit())).unwrap_or(digits.len())
        });
        let token = &rest[at..end];
        match super::reference(token).and_then(|id| case.record_index(g, id).ok()) {
            Some((_, i)) => out.push_str(&i.to_string()),
            None => out.push_str(token),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

impl Enc for () { fn enc(self) -> String { String::new() } }
impl Enc for f32 { fn enc(self) -> String { hex(self) } }
impl Enc for f64 { fn enc(self) -> String { format!("{:016x}", self.to_bits()) } }
impl Enc for bool { fn enc(self) -> String { bit(self) } }
impl Enc for String { fn enc(self) -> String { quoted(&self) } }
impl Enc for &str { fn enc(self) -> String { quoted(self) } }
impl<T: Enc> Enc for Option<T> { fn enc(self) -> String { self.map_or_else(|| "null".into(), Enc::enc) } }
impl Enc for crate::b2::math::Vec2 { fn enc(self) -> String { vec([self.x, self.y]) } }
impl<const N: usize> Enc for [f32; N] { fn enc(self) -> String { vec(self) } }
macro_rules! enc_int { ($($t:ty),*) => { $(impl Enc for $t { fn enc(self) -> String { self.to_string() } })* } }
enc_int!(usize, i32, u8, u16, u32, i64, u64);
/// A List or array: `[a;b]`.
impl<T: Enc> Enc for Vec<T> { fn enc(self) -> String { format!("[{}]", self.into_iter().map(Enc::enc).collect::<Vec<_>>().join(";")) } }
/// A thrown exception (the recorder's `exc:<class>`): the rewrite's error names it.
/// A pair (Vector2Int, ValueTuple<int, int>): `a,b` (game_ret_plain unwraps the game's struct).
impl<A: Enc, B: Enc> Enc for (A, B) { fn enc(self) -> String { format!("{},{}", self.0.enc(), self.1.enc()) } }
impl<T: Enc> Enc for Result<T, String> { fn enc(self) -> String { self.map_or_else(|e| format!("exc:{e}"), Enc::enc) } }

mod fnbiga;
pub(super) mod fnv52;
include!("fncheck_gen.rs");
include!("fncheck_dll.rs");
include!("fncheck_dll2.rs");
include!("fncheck_mods.rs");

/// fnweap: the jukebox's track k as the game names it (its record's `tracks` entry).
fn jukebox_track(c: &With, k: usize) -> Result<String, String> {
    let r = c.c.s.of("ItemScript").find(|r| r.id == c.c.sid).ok_or("no jukebox record")?;
    Ok(r.req("tracks")?.trim_start_matches('[').trim_end_matches(']').split(';').nth(k).ok_or("no track")?.to_string())
}

/// Global's physical chunks as the recording has them (`GlobalChunks`): up to date or stale, and each chunk's entries.
fn load_chunks(g: &mut Game, s: &Snapshot) -> Result<(), String> {
    let Some(r) = s.of("GlobalChunks").next() else { return Ok(()) };
    g.chunksUpToDate = r.req("upToDate")? == "1";
    g.physical_chunks.clear();
    let text = r.req("chunks")?.trim_start_matches('[').trim_end_matches(']');
    for chunk in text.split(';').filter(|c| !c.is_empty()) {
        let (key, ids) = chunk.split_once(':').ok_or("bad chunk")?;
        let (x, y) = key.split_once(',').ok_or("bad chunk key")?;
        let key = (x.parse::<i32>().map_err(|e| e.to_string())?, y.parse::<i32>().map_err(|e| e.to_string())?);
        // A destroyed entry (0): GetPhysicsObjectsNear's `(bool)` skips it.
        let list = ids.split(',').filter(|i| !i.is_empty() && *i != "0").map(|i| phys_by_id(g, i.parse().map_err(|_| format!("bad id {i}"))?)).collect::<Result<Vec<_>, String>>()?;
        g.physical_chunks.insert(key, list);
    }
    Ok(())
}

use crate::game::continuous::Listener;

/// gapA r7: an Awake (`add`: the component's calls taken out first, then AddListener) or OnDestroy (RemoveListener) of a
/// component whose only effect is its OnContinuousUpdate listener; ret `lis=<before>,<after>` (MapConfigProbe.ListenPre).
fn cont_listen(g: &mut Game, c: &Case, add: bool, of: fn(usize) -> Listener) -> Result<String, String> {
    let l = of(c.with(1)?.p_usize(g, 0)?);
    if add {
        g.continuous_remove_listener(l);
    }
    let pre = g.continuous_listener_count(l);
    if add { g.continuous_add_listener(l) } else { g.continuous_remove_listener(l) }
    Ok(format!("lis={pre},{}", g.continuous_listener_count(l)))
}

use crate::game::continuous::Ev;

/// gapA r8: the general listener count (MapConfigProbe.GenPre/ListenPost `lis=<before>,<after>`): the self's subscriptions
/// over every event it listens on, `evs` here plus its OnContinuousUpdate `cont` listener. A registering case (`add`: Awake,
/// Start) starts with all of them taken out. `call` is the rewrite's method on the arena index `i`.
fn gen_listen(g: &mut Game, c: &Case, add: bool, i: usize, evs: &[Ev], cont: Option<Listener>, call: fn(&mut Game, usize)) -> Result<String, String> {
    let id = c.sid;
    let total = |g: &Game| evs.iter().map(|&e| g.listen_count(id, e)).sum::<usize>() + cont.map_or(0, |l| g.continuous_listener_count(l));
    if add {
        for &e in evs {
            g.listen_clear(id, e);
        }
        if let Some(l) = cont {
            g.continuous_remove_listener(l);
        }
    }
    let pre = total(g);
    call(g, i);
    Ok(format!("lis={pre},{}", total(g)))
}

fn arena<T>(v: &[T], id: i32, of: fn(&T) -> i32, what: &str) -> Result<usize, String> {
    v.iter().position(|x| of(x) == id).ok_or_else(|| format!("no {what} entry #{id}"))
}

/// gapC r2: DistanceWireTool/FixedWireTool/BloodWireTool.OnToolChosen (wire_tool_choose) on a tool the recorder added
/// (FnTiny.ToolPre's `tpre=<WireWidth>|<line>`): the self's component exists, the controller's LineRenderer as recorded;
/// the result is the line after the call (" tline=", FnTiny.ToolLine's form) as the rewrite's kind leaves it. The block's
/// _Flow: BloodWireTool's 0, else the recorded one (OnToolChosen leaves it).
/// fnrec r1: <WireTool>.OnJointCreate(joint): use_wire_tool_on_joint_create with the FnCase class's WireKind, on the adopted joint.
fn wire_tool_on_joint_create(g: &mut Game, c: &Case) -> Result<String, String> {
    c.with(1)?;
    let f = c.adopted_joint(g, 0).ok_or("joint arg: not adopted")?;
    let name = &c.s.of("FnCase").next().ok_or("no FnCase")?.name;
    let ty = name.split('.').next().unwrap_or("");
    let kind = super::super::blood_wire::WireKind::of_tool(ty).ok_or(format!("not a wire tool: {ty}"))?;
    g.use_wire_tool_on_joint_create(f, kind);
    VOID
}
/// fnrec r1: <FixedWireTool>.OnJointCreate(joint, worldSpaceEndpos) with the FnCase class's WireKind, on the adopted joint.
fn fixed_wire_tool_on_joint_create(g: &mut Game, c: &Case) -> Result<String, String> {
    c.with(2)?;
    let f = c.adopted_joint(g, 0).ok_or("joint arg: not adopted")?;
    let end = c.arg::<[f32; 2]>(1)?;
    let name = &c.s.of("FnCase").next().ok_or("no FnCase")?.name;
    let ty = name.split('.').next().unwrap_or("");
    let kind = super::super::blood_wire::WireKind::of_tool(ty).ok_or(format!("not a wire tool: {ty}"))?;
    let super::super::BodyOwner::Physical(owner) = g.fixed_joints[f].attached else { return Err("joint on a bare Rigidbody2D".into()) };
    let saved = std::mem::replace(&mut g.wire_tool.kind, kind);
    g.fixed_wire_tool_on_joint_create(owner, f, end);
    g.wire_tool.kind = saved;
    VOID
}
fn wire_on_select_as_self(g: &mut Game, c: &Case) -> Result<String, String> {
    c.with(0)?;
    let kind = c.self_kind(super::super::blood_wire::WireKind::of_tool)?;
    let saved = std::mem::replace(&mut g.wire_tool.kind, kind);
    g.wire_on_select();
    g.wire_tool.kind = saved;
    VOID
}
fn wire_tool_chosen(g: &mut Game, c: &Case) -> Result<String, String> {
    use crate::game::blood_wire::WireKind;
    c.with(0)?;
    let kind = c.self_kind(WireKind::of_tool)?;
    let (ww, pre) = c.field("tpre").and_then(|v| v.split_once('|')).ok_or("no tpre= (a recording before gapC r2)")?;
    if !g.wire_tool.created.contains(&kind) {
        g.wire_tool.created.push(kind);
    }
    g.wire_tool.has_line = pre != "null";
    if ww != "-" {
        g.wire_tool.WireWidth = Some(f(ww));
    }
    g.wire_tool_choose(kind);
    // UseWireTool's (and its kin's) SetColor(_GlowColour) leaves a block that is not empty: an unset _Flow reads 0.
    let flow = if kind == WireKind::Blood {
        "00000000"
    } else if pre == "null" {
        "-"
    } else {
        match pre.rsplit('/').next().unwrap_or("-") {
            "-" if matches!(kind, WireKind::Use(..)) => "00000000",
            f => f,
        }
    };
    let k = g.wire_tool.kind;
    let col = k.color().map(hex).join(",");
    let (caps, stretch, order) = k.tool_line();
    // LineAlignment.Local is 1; LineTextureMode Stretch 0, Tile 1. OnToolChosen leaves the line disabled.
    Ok(format!(" tline=0/{col}/{col}/Top/{}/1/{caps}/{order}/{}/{}/{flow}", hex(g.wire_tool.WireWidth.unwrap_or(k.width())), k.material().rsplit('/').next().unwrap_or(""), if stretch { 0 } else { 1 }))
}

/// fnbatch r3: the ambient grid cells under a TransferHeat case's physical (`AmbientGridCells`, FnTiny.GridCells): each
/// `x:y:Read:Write:CanCreateNeighbours:CanTransferTemperature`, or `x:y:none` for a key World does not hold.
fn grid_cells(s: &Snapshot) -> Result<Vec<((i32, i32), Option<crate::game::temperature::Cell>)>, String> {
    let Some(r) = s.of("AmbientGridCells").next() else { return Ok(Vec::new()) };
    r.req("cells")?.trim_start_matches('[').trim_end_matches(']').split(';').filter(|c| !c.is_empty()).map(|c| {
        let f: Vec<&str> = c.split(':').collect();
        let k = (f[0].parse::<i32>().map_err(|e| e.to_string())?, f[1].parse::<i32>().map_err(|e| e.to_string())?);
        if f.get(2) == Some(&"none") { return Ok((k, None)) }
        let h = |x: &str| u32::from_str_radix(x, 16).map(f32::from_bits).map_err(|e| e.to_string());
        let mut x = crate::game::temperature::Cell::new(h(f[2])?);
        (x.WriteTemperature, x.CanCreateNeighbours, x.CanTransferTemperature) = (h(f[3])?, f[4] == "1", f[5] == "1");
        Ok((k, Some(x)))
    }).collect()
}

fn load_grid_cells(g: &mut Game, s: &Snapshot) -> Result<(), String> {
    // The grid's own fields (GenRun fuzzes them): `rates=ObjectToAir,AirToObject,AmbientTemperatureTransferRate,GridSize`.
    if let Some(v) = s.of("AmbientGridCells").next().and_then(|r| r.get("rates")) {
        let f: Vec<&str> = v.split(',').collect();
        let h = |x: &str| u32::from_str_radix(x, 16).map(f32::from_bits).map_err(|e| e.to_string());
        let a = &mut g.ambient_grid;
        (a.ObjectToAirTransferRate, a.AirToObjectTransferRate, a.AmbientTemperatureTransferRate) = (h(f[0])?, h(f[1])?, h(f[2])?);
        a.GridSize = f[3].parse().map_err(|_| format!("bad GridSize {}", f[3]))?;
    }
    for (k, x) in grid_cells(s)? {
        if let Some(x) = x { g.ambient_grid.World.try_add(k, x); }
    }
    Ok(())
}

/// batch4 r3: FnTiny.CameraShake's record (`shake= root= lpos= nd=`): CameraShakeBehaviour.main's shakeIntensity, its
/// root's position, Global.CameraPosition and CameraListenerBehaviour.NormalisedDistance.
fn load_camera_shake(g: &mut Game, s: &Snapshot) -> Result<(), String> {
    let Some(r) = s.of("CameraShake").next() else { return Ok(()) };
    let f = |k: &str| -> Result<Vec<f32>, String> { r.req(k)?.split(',').map(|x| u32::from_str_radix(x, 16).map(f32::from_bits).map_err(|e| e.to_string())).collect() };
    let v3 = |k: &str| -> Result<[f32; 3], String> { f(k)?.try_into().map_err(|_| format!("CameraShake {k}")) };
    (g.camera.shakeIntensity, g.camera.root, g.camera_control.position, g.camera.normalised_distance) = (f("shake")?[0], v3("root")?, v3("lpos")?, f("nd")?[0]);
    Ok(())
}

/// batch4 r11: FnTiny.UndoHistory's record (`pos= history=[kind:name:ids;...]`): UndoControllerBehaviour's history (the
/// ObjectCreationActions FnTiny seeds; ids a GameObject's Transform id, else the object's) and currentHistoryPosition.
/// batch2 r12: the Human's own DisintegrationCounterBehaviour (`HumanDisintegrationCounter`): DisintegrationCount, the
/// children a later Start replaced, and the subscriptions beyond the first Start's.
fn load_human_counter(g: &mut Game, s: &Snapshot) -> Result<(), String> {
    for r in s.of("HumanDisintegrationCounter") {
        let Some(person) = human_counter_person(g, r.id) else { continue };
        g.people[person].DisintegrationCount = r.req("DisintegrationCount")?.parse().map_err(|_| "HumanDisintegrationCounter DisintegrationCount")?;
        let phys = |g: &Game, id: i32| g.physicals.iter().position(|p| p.id == id).ok_or(format!("HumanDisintegrationCounter: no physical {id}"));
        let first = human_counter_children(g, person);
        let children = list_ids(r.req("children")?).into_iter().map(|id| phys(g, id)).collect::<Result<Vec<_>, _>>()?;
        g.people[person].counter_children = (children != first).then_some(children);
        let mut subs = Vec::new();
        for (id, n) in human_counter_subs(r.req("subs")?) {
            let p = phys(g, id)?;
            subs.extend(std::iter::repeat_n(p, n.saturating_sub(first.contains(&p) as usize)));
        }
        g.people[person].counter_subs = subs;
    }
    Ok(())
}

/// The person whose PersonBehaviour shares the counter's GameObject.
fn human_counter_person(g: &mut Game, id: i32) -> Option<usize> {
    if !g.unsynced.iter().chain(&g.retained).any(|i| i.objects.iter().any(|x| x.components.contains(&id))) {
        return None;
    }
    let own = g.with_component_instance(id, |_, inst, o| inst.objects[o].components.clone());
    g.people.iter().position(|p| own.contains(&p.id))
}

/// The person's DisintegrationCounterBehaviour.children, as physicals.
fn human_counter_children(g: &Game, person: usize) -> Vec<usize> {
    let p = &g.people[person];
    p.counter_children.clone().unwrap_or_else(|| p.Limbs.iter().map(|&l| g.limbs[l].PhysicalBehaviour).chain(p.pseudo_children.iter().copied()).collect())
}

/// `[id;id;...]`
fn list_ids(v: &str) -> Vec<i32> {
    v.trim_matches(['[', ']']).split(';').filter_map(|x| x.parse().ok()).collect()
}

/// `[id:n;...]`
fn human_counter_subs(v: &str) -> Vec<(i32, usize)> {
    v.trim_matches(['[', ']']).split(';').filter_map(|x| x.split_once(':')).filter_map(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))).collect()
}

fn load_undo(g: &mut Game, s: &Snapshot) -> Result<(), String> {
    let Some(r) = s.of("UndoHistory").next() else { return Ok(()) };
    g.undo.currentHistoryPosition = r.req("pos")?.parse().map_err(|_| "UndoHistory pos")?;
    g.undo.history.clear();
    for a in r.req("history")?.trim_matches(['[', ']']).split(';').filter(|a| !a.is_empty()) {
        let p: Vec<&str> = a.splitn(3, ':').collect();
        let ids = p.get(2).map_or(Ok(Vec::new()), |x| x.split(',').filter(|x| !x.is_empty()).map(|x| x.parse::<i32>().map_err(|_| format!("UndoHistory id {x}"))).collect())?;
        match p[0] {
            "Creation" => g.undo.history.push(super::super::undo::ObjectCreationAction { displayName: p[1].replace('_', " "), RelevantObjects: ids }.into()),
            k => return Err(format!("UndoHistory: a {k} action (not loaded)")),
        }
    }
    Ok(())
}

/// The history as FnTiny.UndoHistory writes it.
fn undo_record(g: &Game) -> String {
    use super::super::undo::UndoAction;
    let ids = |v: &[i32]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
    let acts: Vec<String> = g.undo.history.iter().map(|a| match a {
        UndoAction::Creation(a) => format!("Creation:{}:{}", a.displayName.replace(' ', "_"), ids(&a.RelevantObjects)),
        UndoAction::Freeze(a) => format!("Freeze:{}:{}", a.displayName.replace(' ', "_"), a.RelevantObject),
        UndoAction::Paste(a) => format!("Paste:{}:{}", a.displayName.replace(' ', "_"), ids(&a.RelevantObjects)),
    }).collect();
    format!("pos={} history=[{}]", g.undo.currentHistoryPosition, acts.join(";"))
}

fn compare_undo(g: &Game, s: &Snapshot) -> Vec<String> {
    let Some(r) = s.of("UndoHistory").next() else { return Vec::new() };
    let game = format!("pos={} history={}", r.get("pos").unwrap_or(""), r.get("history").unwrap_or(""));
    let ours = undo_record(g);
    if game != ours { vec![format!("UndoHistory: game {game} ours {ours}")] } else { Vec::new() }
}

/// batch4 r12: FnTiny.ToolWheelListeners's record (`list=[<owner id>;...]`): ToolWheelBehaviour.OnToolVisibilityChanged's
/// invocation list, each a ToolInvisibilityTracker's handler written as its owner's (the wire's or pin's) id.
fn tracker_owner(g: &Game, id: i32) -> Result<super::super::tool_invisibility::Owner, String> {
    use super::super::tool_invisibility::Owner;
    if let Some(w) = g.blood_wires.iter().position(|w| w.id == id) { return Ok(Owner::Wire(w)) }
    g.pins.iter().position(|p| p.id == id).map(Owner::Pin).ok_or(format!("listener owner #{id}: no wire or pin"))
}

fn tracker_owner_id(g: &Game, o: super::super::tool_invisibility::Owner) -> i32 {
    use super::super::tool_invisibility::Owner;
    match o { Owner::Wire(w) => g.blood_wires[w].id, Owner::Pin(p) => g.pins[p].id }
}

fn load_tool_wheel_listeners(g: &mut Game, s: &Snapshot) -> Result<(), String> {
    let Some(r) = s.of("ToolWheelListeners").next() else { return Ok(()) };
    let ids: Vec<i32> = r.req("list")?.trim_matches(['[', ']']).split(';').filter(|x| !x.is_empty()).map(|x| x.parse().map_err(|_| format!("ToolWheelListeners {x}"))).collect::<Result<_, _>>()?;
    g.trackers = ids.into_iter().map(|id| tracker_owner(g, id)).collect::<Result<_, _>>()?;
    Ok(())
}

/// gapC r22: FreezeStackController's static stack (`FreezeStack all 0 sf=[<Rigidbody2D id>:<RefInt.Value>;...]`, recorder r22),
/// by the PhysicalBehaviour whose `rigidbody` the body is: (its id, the count). A body no PhysicalBehaviour holds is an error.
fn freeze_stack_record(s: &Snapshot) -> Option<Result<std::collections::BTreeMap<i32, i32>, String>> {
    let r = s.of("FreezeStack").next()?;
    let body = r.get("sf")?.trim_matches(['[', ']']).to_string();
    Some(body.split(';').filter(|x| !x.is_empty()).map(|e| {
        let (rb, n) = e.split_once(':').ok_or(format!("FreezeStack {e}"))?;
        let key = format!("#{rb}");
        let pb = s.of("PhysicalBehaviour").find(|x| x.get("rigidbody").is_some_and(|v| v.ends_with(&key))).ok_or(format!("FreezeStack: no PhysicalBehaviour with Rigidbody2D #{rb}"))?;
        Ok((pb.id, n.parse().map_err(|_| format!("FreezeStack {e}"))?))
    }).collect())
}

fn load_freeze_stack(g: &mut Game, s: &Snapshot) -> Result<(), String> {
    let Some(m) = freeze_stack_record(s) else { return Ok(()) };
    g.freeze_stack = m?.into_iter().map(|(id, n)| Ok((phys_by_id(g, id)?, n))).collect::<Result<_, String>>()?;
    Ok(())
}

fn compare_freeze_stack(g: &Game, s: &Snapshot) -> Vec<String> {
    let Some(m) = freeze_stack_record(s) else { return Vec::new() };
    let game = match m { Ok(m) => m, Err(e) => return vec![e] };
    let ours: std::collections::BTreeMap<i32, i32> = g.freeze_stack.iter().map(|(&p, &n)| (g.physicals[p].id, n)).collect();
    if game != ours { vec![format!("FreezeStack: game {game:?} ours {ours:?}")] } else { Vec::new() }
}

/// gapC r22: ToolWheelBehaviour.CurrentTab (`ToolWheel all 0 tab=<int> tabs=[<Tab>:<activeSelf>;...]`, recorder r22): ours is the
/// Tool's `tab`; each tab object's activeSelf is ours `Tab == tab` (the toolbar view draws them).
fn load_tool_wheel(g: &mut Game, s: &Snapshot) -> Result<(), String> {
    let Some(r) = s.of("ToolWheel").next() else { return Ok(()) };
    let tab = r.req("tab")?.parse().map_err(|_| "ToolWheel tab".to_string())?;
    if let Some(t) = g.tool.as_mut() { t.tab = tab }
    Ok(())
}

fn compare_tool_wheel(g: &Game, pi: &Snapshot, s: &Snapshot) -> Vec<String> {
    let (Some(r), Some(t)) = (s.of("ToolWheel").next(), g.tool.as_ref()) else { return Vec::new() };
    let mut d = Vec::new();
    if r.get("tab") != Some(t.tab.to_string().as_str()) { d.push(format!("ToolWheel tab: game {} ours {}", r.get("tab").unwrap_or("?"), t.tab)) }
    // The tab objects only where a SetActive can show: the game's changed during the call, or ours' tab did (a PI's objects
    // can disagree with its tab: an earlier case's, which the reset leaves).
    let (pre, pre_tab) = pi.of("ToolWheel").next().map_or((None, None), |x| (x.get("tabs"), x.get("tab")));
    if pre != r.get("tabs") || pre_tab != Some(t.tab.to_string().as_str()) {
        let game = r.get("tabs").unwrap_or("").trim_matches(['[', ']']).to_string();
        let ours = game.split(';').filter(|x| !x.is_empty()).map(|e| { let k = e.split(':').next().unwrap_or(""); format!("{k}:{}", u8::from(k == t.tab.to_string())) }).collect::<Vec<_>>().join(";");
        if game != ours { d.push(format!("ToolWheel tabs: game [{game}] ours [{ours}]")) }
    }
    d
}

/// gapC r22: MapLightBehaviour.propertyBlock (`MapLightBehaviour pb=`, FnTiny.MpbOf of the held block, recorder r22), PI into
/// ours' light by id: its floats and vectors (a texture is not named: ours keeps the block's textures).
fn load_map_light_blocks(g: &mut Game, s: &Snapshot) {
    use crate::game::props::Prop;
    // gapC r23: the light's renderer's own block (`rpb=`, FnTiny.MpbOf of the SpriteRenderer: what Awake's GetPropertyBlock reads).
    for r in s.of("MapLightBehaviour") {
        let (Some(m), Some(l)) = (r.get("rpb").filter(|m| *m != "?" && *m != "null"), g.map_lights.iter().position(|l| l.id == r.id)) else { continue };
        let Some(at) = g.map_lights[l].spriteRenderer else { continue };
        let mut block = g.get_property_block(at);
        block.0.retain(|_, p| matches!(p, Prop::Texture(_)));
        if m != "-" {
            for (n, x) in m.split(';').filter_map(|x| x.split_once(':')) {
                let v: Vec<f32> = x[1..].split(',').filter_map(|h| u32::from_str_radix(h, 16).ok().map(f32::from_bits)).collect();
                match (x.as_bytes()[0], v.as_slice()) {
                    (b'f', [f]) if f.to_bits() != 0 => block.set_float(n, *f),
                    (b'v', &[a, b, c, d]) if [a, b, c, d].iter().any(|x| x.to_bits() != 0) => block.set_vector(n, [a, b, c, d]),
                    _ => {}
                }
            }
        }
        g.set_property_block(at, &block);
    }
    // gapC r23: each FuseBoxBehaviour's audioProvider (`FuseBox clip=`, 0: none), and the GameObjects a DebrisComponent was added
    // to (a Clone's comps lists one its prefab object lacks: BulbBehaviour.Break's), by their Transform id.
    for r in s.of("FuseBox") {
        match r.get("clip").and_then(|v| v.parse::<i32>().ok()) {
            Some(c) if c != 0 => { g.fuse_box_clips.insert(r.id, c); }
            _ => { g.fuse_box_clips.remove(&r.id); }
        }
    }
    for r in s.of("Clone") {
        let Some(comps) = r.get("comps") else { continue };
        let comps: Vec<&str> = comps.trim_matches(['[', ']']).split(';').collect();
        if comps.iter().any(|x| x.starts_with("DebrisComponent:"))
            && let Some(t) = comps.iter().find_map(|x| x.strip_prefix("Transform:")?.parse::<i32>().ok())
        {
            g.debris_added.insert(t);
        }
    }
    for r in s.of("MapLightBehaviour") {
        let (Some(m), Some(l)) = (r.get("pb").filter(|m| *m != "?" && *m != "null"), g.map_lights.iter().position(|l| l.id == r.id)) else { continue };
        let block = &mut g.map_lights[l].propertyBlock;
        block.0.retain(|_, p| matches!(p, Prop::Texture(_)));
        if m == "-" { block.0.clear(); continue }
        for (n, x) in m.split(';').filter_map(|x| x.split_once(':')) {
            let v: Vec<f32> = x[1..].split(',').filter_map(|h| u32::from_str_radix(h, 16).ok().map(f32::from_bits)).collect();
            match (x.as_bytes()[0], v.as_slice()) {
                (b'f', [f]) if f.to_bits() != 0 => block.set_float(n, *f),
                (b'v', &[a, b, c, d]) if [a, b, c, d].iter().any(|x| x.to_bits() != 0) => block.set_vector(n, [a, b, c, d]),
                _ => {}
            }
        }
    }
}

/// A PropBlock as mpb_map shows it.
fn block_map(b: &crate::game::props::PropBlock) -> std::collections::BTreeMap<String, String> {
    use crate::game::props::Prop;
    let h = |v: &[f32]| v.iter().map(|x| format!("{:08x}", x.to_bits())).collect::<Vec<_>>().join(",");
    b.0.iter().filter_map(|(n, p)| {
        let x = match p {
            Prop::Float(v) => format!("f{}", h(&[*v])),
            Prop::Vector(v) => format!("v{}", h(v)),
            Prop::Color(c) => format!("v{}", h(&crate::custom_material::linear(*c).to_array())),
            Prop::Texture(_) => "t".into(),
            Prop::VectorArray(_) => return None,
        };
        Some((n.clone(), mpb_norm(&x)))
    }).filter(|(n, x)| !x.is_empty() && n != "_MainTex").collect()
}

fn compare_map_light_blocks(g: &Game, s: &Snapshot) -> Vec<String> {
    let mut d = Vec::new();
    for r in s.of("MapLightBehaviour") {
        let Some(game) = r.get("pb").and_then(mpb_map) else { continue };
        let Some(l) = g.map_lights.iter().position(|l| l.id == r.id) else { continue };
        let game: std::collections::BTreeMap<String, String> = game.into_iter().filter(|(_, x)| !x.is_empty()).collect();
        let ours = block_map(&g.map_lights[l].propertyBlock);
        if game != ours { d.push(format!("MapLightBehaviour #{} propertyBlock: game {game:?} ours {ours:?}", r.id)) }
    }
    d
}

fn compare_tool_wheel_listeners(g: &Game, s: &Snapshot) -> Vec<String> {
    let Some(r) = s.of("ToolWheelListeners").next() else { return Vec::new() };
    let ours = format!("[{}]", g.trackers.iter().map(|&o| tracker_owner_id(g, o).to_string()).collect::<Vec<_>>().join(";"));
    let game = r.get("list").unwrap_or("");
    if game != ours { vec![format!("ToolWheelListeners: game {game} ours {ours}")] } else { Vec::new() }
}

/// A `listener:<owner id>` arg (FnTiny.Fmt of an Action<Visibility> whose Target is a ToolInvisibilityTracker).
fn listener_arg(g: &Game, a: &str) -> Result<super::super::tool_invisibility::Owner, String> {
    tracker_owner(g, a.strip_prefix("listener:").ok_or(format!("arg {a}: no listener:<id>"))?.parse().map_err(|_| format!("arg {a}"))?)
}

/// An `undo:<index>` arg (FnTiny.Fmt): the action's history index, None for one not in the history.
fn undo_arg(a: &str) -> Result<Option<usize>, String> {
    let i: i64 = a.strip_prefix("undo:").ok_or(format!("arg {a}: no undo:<index>"))?.parse().map_err(|_| format!("arg {a}"))?;
    Ok(usize::try_from(i).ok())
}

/// A FindRelevantAction `o`: a GameObject's ref (`@name#<GameObject id>`) as its Transform's id (the Clone row's comps), else the ref's id.
fn undo_object(c: &Case, a: &str) -> Result<i32, String> {
    let id = reference(a).ok_or(format!("arg {a}: no reference"))?;
    Ok(c.s.of("Clone").find(|r| r.id == id).and_then(|r| r.get("comps")).and_then(|x| x.trim_matches(['[', ']']).split(';').find_map(|e| e.strip_prefix("Transform:")?.parse().ok())).unwrap_or(id))
}

// gapB r23: IgnoreCollisionStackController's stack (`IgnoreStack pairs=[a,b:n;..]`, the counts the dump loads): a pair
// missing on one side counts 0.
fn compare_ignore_stack(g: &Game, s: &Snapshot) -> Vec<String> {
    let Some(r) = s.of("IgnoreStack").next() else { return Vec::new() };
    let mut st = std::collections::HashMap::new();
    for x in r.get("pairs").unwrap_or("").trim_matches(['[', ']']).split(';').filter(|x| !x.is_empty()) {
        let Some((p, n)) = x.rsplit_once(':') else { return vec![format!("bad IgnoreStack entry {x}")] };
        let Some((a, b)) = p.split_once(',') else { return vec![format!("bad IgnoreStack pair {x}")] };
        let (Ok(a), Ok(b), Ok(n)) = (a.parse::<i32>(), b.parse::<i32>(), n.parse::<i32>()) else { return vec![format!("bad IgnoreStack entry {x}")] };
        st.insert((a, b), n);
    }
    let mut keys: Vec<(i32, i32)> = st.keys().chain(g.ignore_stack.keys()).copied().collect();
    keys.sort();
    keys.dedup();
    keys.into_iter().filter_map(|k| {
        let (a, b) = (st.get(&k).copied().unwrap_or(0), g.ignore_stack.get(&k).copied().unwrap_or(0));
        (a != b).then(|| format!("IgnoreStack {},{}: game {a} ours {b}", k.0, k.1))
    }).take(4).collect()
}

fn compare_camera_shake(g: &Game, s: &Snapshot) -> Vec<String> {
    let Some(r) = s.of("CameraShake").next() else { return Vec::new() };
    let (a, b) = (r.get("shake").unwrap_or(""), hex(g.camera.shakeIntensity));
    if a != b { vec![format!("CameraShakeBehaviour.shakeIntensity: game {a} ours {b}")] } else { Vec::new() }
}

fn compare_grid_cells(g: &Game, s: &Snapshot) -> Vec<String> {
    let Ok(cells) = grid_cells(s) else { return vec!["bad AmbientGridCells".into()] };
    let w = &g.ambient_grid.World;
    cells.into_iter().filter_map(|(k, x)| {
        let ours = w.find(k).map(|i| w.cell(i));
        let f = |c: Option<&crate::game::temperature::Cell>| c.map_or("none".to_string(), |c| format!("{}:{}:{}:{}", hex(c.ReadTemperature), hex(c.WriteTemperature), c.CanCreateNeighbours as u8, c.CanTransferTemperature as u8));
        let (a, b) = (f(x.as_ref()), f(ours));
        (a != b).then(|| format!("grid cell {k:?}: game {a} ours {b}"))
    }).collect()
}

/// fnweap: the arena index of ReactorA5ArcBehaviour `id`.
fn arc_index(g: &Game, id: i32) -> Result<usize, String> {
    g.reactor_a5_arcs.iter().position(|x| x.id == id).ok_or_else(|| format!("no ReactorA5ArcBehaviour #{id}"))
}

fn phys_by_id(g: &Game, id: i32) -> Result<usize, String> {
    g.physicals.iter().position(|p| p.id == id).ok_or_else(|| format!("no PhysicalBehaviour #{id}"))
}

/// A Transform arg: (its Global.PhysicalObjectsInWorldByTransform entry, the PhysicalBehaviour on its GameObject).
fn transform_physical(g: &Game, a: &str) -> Result<(Option<usize>, Option<usize>), String> {
    let id = reference(a).ok_or("null Transform")?;
    let own = g.physicals.iter().position(|p| p.transform_id == id && !p.destroyed);
    Ok((own.filter(|&p| g.physicals[p].in_world), own))
}

/// BigProbeB.Fmt's RaycastHit2D: "ray:<collider id>,<point x>,<point y>,<normal x>,<normal y>,<distance>".
fn ray_arg(g: &Game, a: &str) -> Result<crate::game::explosion::RayHit2D, String> {
    let v: Vec<&str> = a.strip_prefix("ray:").ok_or("not a ray arg")?.split(',').collect();
    let id: i32 = v[0].parse().map_err(|_| "ray collider id")?;
    let h = |i: usize| u32::from_str_radix(v[i], 16).map(f32::from_bits).map_err(|_| "ray float".to_string());
    let f = g.world.fixtures.iter().flatten().find(|f| f.collider.id == id).ok_or(format!("no collider #{id}"))?;
    let v2 = crate::b2::math::Vec2::new;
    Ok(crate::game::explosion::RayHit2D { collider: id, game_object: f.collider.game_object, body: f.body, point: v2(h(1)?, h(2)?), normal: v2(h(3)?, h(4)?), distance: h(5)?, fraction: 0.0 })
}

/// MapConfigProbe.Laser: "<hit 0/1>,<hit point>,<hit normal>,<hit distance>,<point>,<physicalBehaviour id or 0>,<rayDepth>,<totalDistance>".
fn laser_words(g: &Game, h: &crate::game::detector::LaserHit) -> String {
    let (hp, hn, hd) = h.hit.as_ref().map_or(([0.0; 2], [0.0; 2], 0.0), |x| ([x.point.x, x.point.y], [x.normal.x, x.normal.y], x.distance));
    let id = h.physical.map_or(0, |p| g.physicals[p].id);
    format!("{},{},{},{},{},{},{},{},{},{},{}", bit(h.hit.is_some()), hex(hp[0]), hex(hp[1]), hex(hn[0]), hex(hn[1]), hex(hd), hex(h.point[0]), hex(h.point[1]), id, h.rayDepth, hex(h.totalDistance))
}

/// A "lasers:" list arg: the probe's no-hit entries (hit 0, no physicalBehaviour).
fn laser_list(a: &str) -> Result<Vec<crate::game::detector::LaserHit>, String> {
    let a = a.strip_prefix("lasers:").ok_or_else(|| format!("not a lasers arg: {a}"))?;
    a.split('/').filter(|e| !e.is_empty()).map(|e| {
        let w: Vec<&str> = e.split(',').collect();
        let fl = |i: usize| u32::from_str_radix(w[i], 16).map(f32::from_bits).map_err(|_| format!("laser entry {e}"));
        if w.len() != 11 || w[0] != "0" || w[8] != "0" { return Err(format!("laser entry with a hit: {e}")) }
        let mut h = crate::game::detector::LaserHit::new(None, [fl(6)?, fl(7)?], None, w[9].parse().map_err(|_| format!("laser entry {e}"))?);
        h.totalDistance = fl(10)?;
        Ok(h)
    }).collect()
}

/// gapB r19: a "laser:" arg (MapConfigProbe.Laser's 11 words) as a LaserHit; a hit is a RayHit2D on the physical's body
/// at the hit point (FitsCriteria only tests it for a value).
fn laser_hit_arg(g: &Game, a: &str) -> Result<crate::game::detector::LaserHit, String> {
    let w: Vec<&str> = a.strip_prefix("laser:").ok_or_else(|| format!("not a laser arg: {a}"))?.split(',').collect();
    if w.len() != 11 { return Err(format!("laser arg: {a}")) }
    let fl = |i: usize| u32::from_str_radix(w[i], 16).map(f32::from_bits).map_err(|_| format!("laser arg {a}"));
    let id: i32 = w[8].parse().map_err(|_| format!("laser arg {a}"))?;
    let p = if id == 0 { None } else { Some(phys_by_id(g, id)?) };
    let v2 = crate::b2::math::Vec2::new;
    // FitsCriteria tests the physicalBehaviour before the hit: a hit without one answers false either way, so it is None here.
    let hit = if w[0] == "1" && let Some(q) = p {
        Some(crate::game::explosion::RayHit2D { collider: 0, game_object: 0, body: g.physicals[q].rigidbody.body, point: v2(fl(1)?, fl(2)?), normal: v2(fl(3)?, fl(4)?), distance: fl(5)?, fraction: 0.0 })
    } else { None };
    let mut h = crate::game::detector::LaserHit::new(hit, [fl(6)?, fl(7)?], p, w[9].parse().map_err(|_| format!("laser arg {a}"))?);
    h.totalDistance = fl(10)?;
    Ok(h)
}

use crate::game::mods::mod_api;
/// gapB r19: a ModAPI.Invoke<X> case: the sender's id (arg 0, an Object ref) and a subscriber on `e` for the call.
fn mod_rx<T: 'static>(e: &'static crate::game::mods::ModApiEvent<T>, c: &Case, arg: T, show: fn(&T) -> String) -> Result<String, String> {
    c.with(2)?;
    let sender = reference(c.args[0]).unwrap_or(0);
    let rx = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let r2 = rx.clone();
    let h: std::sync::Arc<dyn Fn(i32, &T) + Send + Sync> = std::sync::Arc::new(move |s, a| r2.lock().unwrap().push(format!("{s},{}", show(a))));
    e.add(h.clone());
    e.invoke(sender, &arg);
    e.remove(&h);
    let v = rx.lock().unwrap().join(";");
    Ok(format!(" mrx={v}"))
}
fn mod_id(c: &Case) -> Result<i32, String> {
    Ok(reference(c.args.get(1).ok_or("no args arg")?).unwrap_or(0))
}
fn show_id(a: &i32) -> String {
    a.to_string()
}
/// A recorded UserSpawnEventArgs: "USE(<instance ref or null>,<asset name or null>)".
fn mod_spawn(c: &Case) -> Result<crate::game::mods::UserSpawnEventArgs, String> {
    let a = c.args.get(1).ok_or("no args arg")?;
    let (i, n) = a.strip_prefix("USE(").and_then(|x| x.strip_suffix(')')).and_then(|x| x.rsplit_once(',')).ok_or_else(|| format!("not a UserSpawnEventArgs: {a}"))?;
    Ok(crate::game::mods::UserSpawnEventArgs::new(reference(i).unwrap_or(0), (n != "null").then(|| n.to_string())))
}
fn show_spawn(a: &crate::game::mods::UserSpawnEventArgs) -> String {
    format!("{}:{}", a.Instance, a.SpawnableAsset.as_deref().unwrap_or("null").replace(' ', "_"))
}

fn hex(v: f32) -> String {
    format!("{:08x}", v.to_bits())
}
fn vec<const N: usize>(v: [f32; N]) -> String {
    v.iter().map(|x| hex(*x)).collect::<Vec<_>>().join(",")
}
fn vec3(v: [f32; 2]) -> String {
    vec([v[0], v[1], 0.0])
}
fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace(' ', "_"))
}
/// OnImpactCreated's gm: None when its GameObject has no BloodImpactBehaviour (its Clone record's comps).
fn impact_gm(g: &Game, c: &Case) -> Result<Option<usize>, String> {
    let id = reference(c.args.first().ok_or("no gm")?).ok_or("null gm")?;
    // A pooled BloodPuff has its PoolObject record but no Clone row (fnb1r6a_gen).
    let puff = c.s.of("PoolObject").any(|o| o.name == "BloodPuff_poolable_pool" && o.id == id);
    if puff || c.s.of("Clone").find(|r| r.id == id).ok_or_else(|| format!("no Clone #{id}"))?.get("comps").is_some_and(|v| v.contains("BloodImpactBehaviour:")) {
        // A BloodPuff: the loader makes blood_puffs in the pool's PoolObject record order (createdObjects), each record's id
        // its GameObject's.
        let k = c.s.of("PoolObject").filter(|o| o.name == "BloodPuff_poolable_pool").position(|o| o.id == id).ok_or_else(|| format!("BloodPuff #{id}: no PoolObject record"))?;
        return g.blood_puff_pool.as_ref().and_then(|p| p.created.get(k).copied()).map(Some).ok_or_else(|| format!("BloodPuff #{id}: not loaded"));
    }
    Ok(None)
}
/// batch1 r10: the case's chainsaw with its PI record's softConnections loaded (FrictionJoint2D joints rebuilt: no record holds
/// them; a `null` joint is a destroyed one) and FnTiny's " csid=" identities; the instance id counter is kept.
fn chainsaw_with_connections(g: &mut Game, c: &Case) -> Result<usize, String> {
    let i = g.chainsaws.iter().position(|x| x.id == c.sid).ok_or("no chainsaws entry")?;
    let saw = g.chainsaws[i].physical;
    g.chainsaws[i].softConnections = pi_soft_connections(g, c, saw)?;
    csid_identities(g, c)?;
    Ok(i)
}
/// fc41 r4: the self's PI softConnections (any saw: Chainsaw, EnergySword, SharpOnAllSides), joints rebuilt to `saw`; with
/// fc41 r5: a PyroPowerTool's entity (its Awake's pyroEntity) from the self's record when ours loaded none (not the current tool).
fn pyro_from_record(g: &mut Game, c: &Case) -> Result<(), String> {
    if g.pyro_tool.is_some() {
        return Ok(());
    }
    let Some(id) = c.s.of("PyroPowerTool").find(|r| r.id == c.sid).and_then(|r| r.get("pyroEntity")).and_then(reference) else { return Ok(()) };
    let entity = g.pyro_entities.iter().position(|x| x.id == id).ok_or_else(|| format!("pyroEntity #{id} not loaded"))?;
    g.pyro_tool = Some(crate::game::pyro_tool::PyroTool { entity });
    Ok(())
}
/// an " stpre=sc:" (FnTiny.R4Of) each joint's maxForce from it.
fn pi_soft_connections(g: &mut Game, c: &Case, saw: usize) -> Result<crate::game::soft_connect::SoftConnections, String> {
    let r = c.s.of("ItemScript").find(|r| r.id == c.sid).ok_or("no saw record")?;
    let list = r.req("softConnections")?.trim_start_matches('{').trim_end_matches('}');
    let next = g.next_instance_id;
    let mut conns = crate::game::soft_connect::SoftConnections::default();
    for e in super::split_top(list, ';').into_iter().filter(|e| !e.is_empty()) {
        // `<key>:SoftConnection(joint=.. phys=.. coll=.. shouldBeDeleted=..)`: the fields after the '('.
        let body = e.split_once('(').map_or(e, |x| x.1);
        let get = |k: &str| body.split(' ').find_map(|w| w.strip_prefix(k)).map(|v| v.strip_suffix(')').unwrap_or(v)).ok_or_else(|| format!("softConnections entry without {k}: {e}"));
        let phys = phys_by_id(g, reference(get("phys=")?).ok_or("null phys")?)?;
        let coll = reference(get("coll=")?).unwrap_or(0);
        let f = g.add_friction_joint_component(phys);
        g.fixed_joint_set_connected_body(f, Some(crate::game::BodyOwner::Physical(saw)));
        if reference(get("joint=")?).is_none() {
            g.fixed_joint_destroy(f);
        }
        let mut sc = crate::game::soft_connect::SoftConnection::new(f, phys, coll);
        sc.should_be_deleted = get("shouldBeDeleted=")? == "1";
        conns.add(phys, sc);
    }
    g.next_instance_id = next;
    for e in st_pre(c).and_then(|v| v.strip_prefix("sc:")).unwrap_or("").split(';').filter(|e| !e.is_empty()) {
        let [id, mf] = e.split('/').collect::<Vec<_>>()[..] else { return Err(format!("bad sc stpre {e}")) };
        let p = phys_by_id(g, id.parse().map_err(|_| format!("sc id {id}"))?)?;
        if let (Some(sc), Ok(b)) = (conns.get(p).copied(), u32::from_str_radix(mf, 16)) {
            g.friction_joint_set_max_force(sc.joint, f32::from_bits(b));
        }
    }
    Ok(conns)
}
fn sharp_with_connections(g: &mut Game, c: &Case) -> Result<usize, String> {
    let s = g.sharp_on_all_sides.iter().position(|x| x.id == c.sid).ok_or("no sharp_on_all_sides entry")?;
    let saw = g.sharp_on_all_sides[s].physical;
    g.sharp_on_all_sides[s].softConnections = pi_soft_connections(g, c, saw)?;
    Ok(s)
}
fn sharp_state(g: &Game, s: usize) -> String {
    soft_state(g, &g.sharp_on_all_sides[s].softConnections)
}
/// fc41 r4: FnTiny.R4Of's "sc:" of a saw's connections: victim id / joint maxForce, by id.
fn soft_state(g: &Game, conns: &crate::game::soft_connect::SoftConnections) -> String {
    let mut e: Vec<(i32, String)> = conns.keys().into_iter().map(|k| {
        let sc = conns.get(k).unwrap();
        let mf = if g.soft_joint_alive(sc.joint) { hex(g.fixed_joints[sc.joint].friction.map_or(0.0, |f| f.0)) } else { "-".into() };
        (g.physicals[sc.phys].id, mf)
    }).collect();
    e.sort();
    format!(" st=sc:{}", e.iter().map(|(id, mf)| format!("{id}/{mf}")).collect::<Vec<_>>().join(";"))
}
/// fc41 r4: an OnAfterDeserialise case's victims (" svpre=" Guid N hex) and objects (every Transform under " svroot=", the
/// list's GameObject), with the " csid=" identities.
fn r4_victims(g: &mut Game, c: &Case) -> Result<(Vec<u128>, Vec<i32>), String> {
    csid_identities(g, c)?;
    let sv = c.field("svpre").unwrap_or("").split(',').filter(|x| !x.is_empty()).map(|h| u128::from_str_radix(h, 16).map_err(|_| format!("svpre {h}"))).collect::<Result<Vec<_>, _>>()?;
    let root: i32 = c.field("svroot").ok_or("no svroot= (a recording before fc41 r4)")?.parse().map_err(|_| "svroot")?;
    let objects = (0..g.physicals.len()).filter(|&p| g.root_id(p) == root).map(|p| g.physicals[p].transform_id).collect();
    Ok((sv, objects))
}
/// fc41 r4: ImmobilityField joints from " stpre=imf:<collider>/<maxForce>/<anchor x>,<y>;.." (FnTiny.R4Seed captured them with
/// OnTriggerEnter2D), built as it does without its clip and message; then the state after `f`.
fn immobility_entry(g: &mut Game, c: &Case, f: impl FnOnce(&mut Game, usize)) -> Result<String, String> {
    let i = g.immobility_fields.iter().position(|x| x.id == c.sid).ok_or("no immobility_fields entry")?;
    let Some(pre) = st_pre(c) else { f(g, i); return VOID };
    let p = g.immobility_fields[i].physical;
    let next = g.next_instance_id;
    for e in pre.strip_prefix("imf:").ok_or(format!("bad imf stpre {pre}"))?.split(';').filter(|e| !e.is_empty()) {
        let w: Vec<&str> = e.split(['/', ',']).collect();
        let [coll, mf, ax, ay] = w[..] else { return Err(format!("bad imf stpre {e}")) };
        let coll: i32 = coll.parse().map_err(|_| format!("imf coll {coll}"))?;
        let b = |v: &str| u32::from_str_radix(v, 16).map(f32::from_bits).map_err(|_| format!("imf {v}"));
        let go = g.world.collider_game_object(coll).ok_or(format!("imf: no GameObject for collider {coll}"))?;
        let v = g.physicals.iter().position(|q| !q.destroyed && q.all_colliders.contains(&coll)).ok_or(format!("imf: no physical for collider {coll}"))?;
        let j = g.add_friction_joint_component(p);
        g.fixed_joint_set_auto_configure(j, false);
        g.fixed_joint_set_enable_collision(j, true);
        g.fixed_joint_set_connected_body(j, Some(crate::game::BodyOwner::Physical(v)));
        g.friction_joint_set_max_force(j, b(mf)?);
        g.friction_joint_set_max_torque(j, b(mf)?);
        g.fixed_joint_set_anchor(j, [b(ax)?, b(ay)?]);
        g.fixed_joint_set_connected_anchor(j, [0.0, 0.0]);
        g.immobility_fields[i].joints.add(go, crate::game::immobility_field::Capture { joint: j, coll, phys: v });
    }
    g.next_instance_id = next;
    f(g, i);
    let mut e: Vec<(i32, String)> = g.immobility_fields[i].joints.keys().into_iter().map(|k| {
        let cp = *g.immobility_fields[i].joints.get(k).unwrap();
        let s = if g.soft_joint_alive(cp.joint) { let fj = &g.fixed_joints[cp.joint]; format!("{}/{},{}", hex(fj.friction.map_or(0.0, |f| f.0)), hex(fj.state.anchor.x), hex(fj.state.anchor.y)) } else { "-".into() };
        (cp.coll, s)
    }).collect();
    e.sort();
    Ok(format!(" st=imf:{}", e.iter().map(|(id, s)| format!("{id}/{s}")).collect::<Vec<_>>().join(";")))
}
/// FnTiny's " csid=<Transform id>:<UniqueIdentity N hex>,..": the SerialisableIdentities no record holds.
fn csid_identities(g: &mut Game, c: &Case) -> Result<(), String> {
    for kv in c.field("csid").unwrap_or("").split(',').filter(|x| !x.is_empty()) {
        let (t, h) = kv.split_once(':').ok_or("csid entry")?;
        g.identities.insert(t.parse().map_err(|_| "csid id")?, u128::from_str_radix(h, 16).map_err(|_| "csid guid")?);
    }
    Ok(())
}
/// FnTiny.OutlineFmt: `outline:` each rectangle's Center, Size, Pivot (Center: both callers pass it) and AngleDegrees as hex.
fn outline_fmt(rs: &[crate::game::contraption::RotatedRectangle]) -> String {
    let w: Vec<String> = rs.iter().map(|r| [r.Center[0], r.Center[1], r.Size[0], r.Size[1], r.Center[0], r.Center[1], r.AngleDegrees].map(hex).join(",")).collect();
    format!("outline:{}", w.join(";"))
}

/// The recorder's ActivationPropagation: `signal` (Direct, channel 0, no Target) or `signal:<direct>,<channel>,<Target's
/// GameObject id, 0 for null>`.
pub(super) fn signal_arg(arg: &str) -> Result<(bool, u16, i32), String> {
    match arg.strip_prefix("signal") {
        Some("") => Ok((true, 0, 0)),
        Some(r) => {
            let w: Vec<&str> = r.trim_start_matches(':').split(',').collect();
            let [d, ch, t] = w[..] else { return Err(format!("bad signal {arg}")) };
            Ok((d == "1", ch.parse().map_err(|e| format!("signal channel {ch}: {e}"))?, t.parse().map_err(|e| format!("signal target {t}: {e}"))?))
        }
        None => Err(format!("not a signal: {arg}")),
    }
}
/// gapB r23: a UseWireBehaviour case's wire and its signal (`signal` or GenRun's `signal:<direct>,<channel>,<target>`;
/// the target is not read on these paths).
fn use_wire_signal(g: &mut Game, c: &Case) -> Result<(usize, crate::game::signal::Act), String> {
    if c.args.len() > 1 { return Err(format!("arity: 1 arg, the game passed {}", c.args.len())) }
    let (direct, channel, _) = signal_arg(c.args.first().copied().unwrap_or("signal"))?;
    let w = c.record_index(g, c.sid)?.1;
    Ok((w, g.signal_direct(direct, channel)))
}
/// fc41 r3: a signal entry point's " st=sg:<ActivationPropagation.CurrentlyActiveSignals>" (FnTiny.StateOf): the game's count
/// before the call (" stpre=", the arg already made) plus the traversal sets ours took in the call (`n0`: ours before it).
/// Nothing without a stpre (recordings before fc41 r3).
fn signal_sets_state(g: &Game, c: &Case, n0: usize) -> Result<String, String> {
    let Some(pre) = st_pre(c) else { return VOID };
    let pre: usize = pre.strip_prefix("sg:").and_then(|v| v.parse().ok()).ok_or(format!("bad sg stpre {pre}"))?;
    Ok(format!(" st=sg:{}", pre + g.signals.currently_active() - n0))
}
fn signal_entry(g: &mut Game, c: &Case, f: impl FnOnce(&mut Game, usize, &crate::game::signal::Act)) -> Result<String, String> {
    let (w, a) = use_wire_signal(g, c)?;
    let n0 = g.signals.currently_active();
    f(g, w, &a);
    signal_sets_state(g, c, n0)
}
/// fc41 r3: PinBehaviour OnEnable/OnDisable/SyncChildPos from the recorder's opposite state (" stpre=pin:<SpriteChild active, 2:
/// none>,<ParentConstraint source 0 translation offset>"), the call, then the same after (" st=pin:").
fn pin_state_entry(g: &mut Game, c: &Case, f: impl FnOnce(&mut Game, usize)) -> Result<String, String> {
    let c = c.with(1)?;
    let p = c.p_usize(g, 0)?;
    let Some(pre) = st_pre(c.c) else { f(g, p); return VOID };
    let w: Vec<&str> = pre.strip_prefix("pin:").ok_or(format!("bad pin stpre {pre}"))?.split(',').collect();
    let [a, x, y, z] = w[..] else { return Err(format!("bad pin stpre {pre}")) };
    if let Some(r) = g.pins[p].renderer { g.renderers[r].visible = a == "1"; }
    if let Some(k) = g.pins[p].constraint { g.parent_constraints[k].sources[0].translation_offset = [x, y, z].map(|v| f32::from_bits(u32::from_str_radix(v, 16).unwrap_or(0))); }
    f(g, p);
    let a = g.pins[p].renderer.map_or("2".into(), |r| bit(g.renderers[r].visible));
    let o = g.pins[p].constraint.map_or([0.0; 3], |k| g.parent_constraints[k].sources[0].translation_offset);
    Ok(format!(" st=pin:{a},{},{},{}", hex(o[0]), hex(o[1]), hex(o[2])))
}
fn sorting_neighbour(c: &Case, above: bool) -> Result<String, String> {
    if c.args.len() != 2 { return Err(format!("arity: 2 args, the game passed {}", c.args.len())) }
    let v: i32 = c.args[0].parse().map_err(|_| format!("layervalue {}", c.args[0]))?;
    let (ok, id) = super::super::layer_editor::layer_neighbour(v, above);
    Ok(format!("{} out={v};SortingLayer(m_Id={id})", bit(ok)))
}
fn bit(b: bool) -> String {
    (if b { "1" } else { "0" }).into()
}
const VOID: Result<String, String> = Ok(String::new());
/// batch3 r4: the game's transform.position of an ObjectGradientAnimation case (the recorder's `tpos=`) against the rewrite's.
fn gradient_tpos(g: &Game, c: &Case, v: usize) -> Result<(), String> {
    let Some(tp) = c.s.of("FnCase").find(|r| r.id == c.id).and_then(|r| r.get("tpos")) else { return Ok(()) };
    let (game, ours) = (floats::<3>(tp), g.object_gradient_position(v));
    if game.map(f32::to_bits) != ours.map(f32::to_bits) {
        // batch3 r6: our anchor chain (root first) for the diff against the PI's Clone records.
        let chain = g.object_gradient_nodes(v).iter().map(|n| format!("t={:08x},{:08x} s={:08x},{:08x} q={:08x}", n.t[0].to_bits(), n.t[1].to_bits(), n.s[0].to_bits(), n.s[1].to_bits(), n.q[3].to_bits())).collect::<Vec<_>>().join(" | ");
        return Err(format!("transform.position: game {game:?} ({tp}), ours {ours:?} chain {chain}"));
    }
    Ok(())
}
fn power_gauge_of(g: &Game, c: &Case) -> Result<usize, String> {
    match g.behaviour_of.get(&c.sid) { Some(&(_, super::super::Behaviour::PowerGauge(i))) => Ok(i), _ => Err("no PowerGauge".into()) }
}
/// ResetTransformBehaviour's (ResetOnStart, ToReset) from its ItemScript record.
fn reset_transform_fields(c: &Case) -> Result<(bool, i64), String> {
    let r = c.s.of("ItemScript").find(|r| r.id == c.sid).ok_or("no ResetTransformBehaviour record")?;
    let t = r.get("ToReset").and_then(|v| v.parse().ok()).ok_or("no ToReset")?;
    Ok((r.get("ResetOnStart") == Some("1"), t))
}

/// The case's FirearmAttachmentBehaviour (self=).
fn attachment_of(g: &Game, c: &Case) -> Result<usize, String> {
    g.firearm_attachments.iter().position(|x| x.id == c.sid).ok_or_else(|| "no firearm_attachments entry".into())
}

/// batch2 r4: ExplosiveAttachmentBehaviour.OnConnect/OnDisconnect with its firearm's emitter state from em= (BigProbeB.XaEm:
/// the BallisticsEmitter plus ExplosiveRoundParams), the same after the call as the void method's ret. No em=: no firearm (the
/// call throws) or the fnb2e cases.
fn explosive_em(g: &mut Game, c: &Case, call: fn(&mut Game, usize)) -> Result<String, String> {
    let a = attachment_of(g, c)?;
    let em = c.field("em");
    let fi = g.firearm_attachments[a].firearm;
    if let Some(v) = em {
        let f = fi.ok_or("em= but no firearm")?;
        super::fnbigb::em_xp_load(g.firearms[f].emitter.as_mut().ok_or("no emitter")?, v)?;
    }
    call(g, a);
    Ok(match (em, fi) {
        (Some(_), Some(f)) => format!("em={}", super::fnbigb::em_xp_text(g.firearms[f].emitter.as_ref().ok_or("no emitter")?)),
        _ => String::new(),
    })
}

/// batch2 r4: BigProbeB.LeState ("<Instance id>,<active>,<position x,y,z>,<Targets>,<selection>,<LayerDisplay>,<OrderDisplay>") loaded into
/// the rewrite's LayerEditor and the tool's rawSelection; Some(()) when the case had one.
fn le_load(g: &mut Game, c: &Case) -> Result<Option<()>, String> {
    let Some(em) = c.field("em") else { return Ok(None) };
    let v: Vec<&str> = em.split(',').collect();
    if v.len() != 9 {
        return Err(format!("em= has {} values", v.len()));
    }
    let f = |k: usize| u32::from_str_radix(v[k], 16).map(f32::from_bits).map_err(|_| format!("em {k}"));
    let ids = |g: &Game, s: &str| -> Result<Vec<usize>, String> { if s == "-" { Ok(Vec::new()) } else { s.split(':').map(|x| phys_by_id(g, x.parse().map_err(|_| format!("id {x}"))?)).collect() } };
    let targets = ids(g, v[5])?;
    let sel = ids(g, v[6])?;
    // One way to load a selection: the snapshot's tool records (Dump.ForceTool; from_snapshot's load_tool, as batch4's
    // SelectionController.get_Main) when the case has them; em= must agree. Older recordings (fnb2g) hold no tool records:
    // the selection then gets a default tool to sit on (GetRelevantTargets reads only rawSelection).
    match g.tool.as_ref() {
        Some(t) if t.rawSelection != sel => return Err(format!("em= selection {sel:?} but the tool records hold {:?}", t.rawSelection)),
        Some(_) => {}
        None if sel.is_empty() => {}
        None => g.tool = Some(super::super::tool::Tool { rawSelection: sel, ..Default::default() }),
    }
    let le = &mut g.layer_editor;
    le.id = v[0].parse().map_err(|_| "em 0")?;
    le.container_active = v[1] == "1";
    le.container_position = [f(2)?, f(3)?, f(4)?];
    le.Targets = targets;
    le.LayerDisplay = v[7].into();
    le.OrderDisplay = v[8].into();
    Ok(Some(()))
}
/// gapB r1: BigProbeB.LvState's text escapes ('\\' '\n' '\r' '\s' space '\c' comma '\e' '=').
fn lv_esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r").replace(' ', "\\s").replace(',', "\\c").replace('=', "\\e")
}
fn lv_unesc(s: &str) -> String {
    let (mut out, mut it) = (String::new(), s.chars());
    while let Some(ch) = it.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        out.push(match it.next() { Some('n') => '\n', Some('r') => '\r', Some('s') => ' ', Some('c') => ',', Some('e') => '=', Some(x) => x, None => '\\' });
    }
    out
}
/// LimbDamageSourceStats.text as Refresh writes it from the rewrite's three parts (the scene's own text while the first two are empty).
fn lv_stats_text(s: &[String; 3]) -> String {
    if s[0].is_empty() && s[1].is_empty() { s[2].clone() } else { format!("<b>{}</b>\n<color=#FF5555>{}</color>{}", s[0], s[1], s[2]) }
}
/// LimbStatusViewBehaviour from em= "<active>,<t>,<limb ids>,<stats text>,<blood text>,<4 bar scales>".
fn lv_load(g: &mut Game, c: &Case) -> Result<(), String> {
    let em = c.field("em").ok_or("no em=: the UI singleton LimbStatusViewBehaviour's state is not in the recording")?;
    let v: Vec<&str> = em.split(',').collect();
    if v.len() != 9 {
        return Err(format!("em= has {} values", v.len()));
    }
    let f = |k: usize| u32::from_str_radix(v[k], 16).map(f32::from_bits).map_err(|_| format!("em {k}"));
    let limbs = if v[2] == "-" { Vec::new() } else { v[2].split(':').map(|x| { let id: i32 = x.parse().map_err(|_| format!("limb id {x}"))?; g.limbs.iter().position(|l| l.id == id).ok_or(format!("no limb #{id}")) }).collect::<Result<Vec<_>, String>>()? };
    let stats = lv_unesc(v[3]);
    let lv = &mut g.limb_view;
    (lv.active, lv.t, lv.limbs) = (v[0] == "1", f(1)?, limbs);
    // The game's one text back into the rewrite's three parts.
    lv.stats = match stats.strip_prefix("<b>").and_then(|s| s.split_once("</b>\n<color=#FF5555>")).and_then(|(a, rest)| rest.split_once("</color>").map(|(b, c)| (a, b, c))) {
        Some((a, b, c)) => [a.into(), b.into(), c.into()],
        None => [String::new(), String::new(), stats.clone()],
    };
    lv.blood = lv_unesc(v[4]);
    lv.bars = [f(5)?, f(6)?, f(7)?, f(8)?];
    Ok(())
}
fn lv_after(g: &Game) -> String {
    let lv = &g.limb_view;
    let ids = if lv.limbs.is_empty() { "-".to_string() } else { lv.limbs.iter().map(|&l| if g.physicals[g.limbs[l].PhysicalBehaviour].destroyed { "0".into() } else { g.limbs[l].id.to_string() }).collect::<Vec<_>>().join(":") };
    let bars = lv.bars.iter().map(|x| format!("{:08x}", x.to_bits())).collect::<Vec<_>>().join(",");
    format!("em={},{:08x},{ids},{},{},{bars}", bit(lv.active), lv.t.to_bits(), lv_esc(&lv_stats_text(&lv.stats)), lv_esc(&lv.blood))
}
/// The ret with the LayerEditor's state after the call as BigProbeB.After appends it.
fn le_after(g: &Game, em: Option<()>, ret: String) -> String {
    if em.is_none() {
        return ret;
    }
    let ids = |l: &[usize]| if l.is_empty() { "-".to_string() } else { l.iter().map(|&p| if g.physicals[p].destroyed { "0".to_string() } else { g.physicals[p].id.to_string() }).collect::<Vec<_>>().join(":") };
    let hx = |x: f32| format!("{:08x}", x.to_bits());
    let le = &g.layer_editor;
    let sel = g.tool.as_ref().map_or(Vec::new(), |t| t.rawSelection.clone());
    let s = [le.id.to_string(), (if le.container_active { "1" } else { "0" }).into(), hx(le.container_position[0]), hx(le.container_position[1]), hx(le.container_position[2]),
        ids(&le.Targets), ids(&sel), le.LayerDisplay.clone(), le.OrderDisplay.clone()].join(",");
    if ret.is_empty() { format!("em={s}") } else { format!("{ret} em={s}") }
}

/// GearManagerBehaviour.Gears from the FnCase's em= ("<Capacity>,<ids ':'-joined, '-' empty>").
fn gears_load(g: &mut Game, c: &Case) -> Result<(), String> {
    let em = c.field("em").ok_or("no em=")?;
    let (cap, ids) = em.split_once(',').ok_or("bad em=")?;
    let cap: usize = cap.parse().map_err(|_| "em capacity")?;
    let ids: Vec<i32> = if ids == "-" { Vec::new() } else { ids.split(':').map(|x| x.parse().map_err(|_| "em id".to_string())).collect::<Result<_, _>>()? };
    g.gears.size = ids.len();
    g.gears.items = ids.into_iter().map(Some).collect();
    g.gears.items.resize(cap.max(g.gears.size), None);
    Ok(())
}

/// The void method's ret as BigProbeB.After writes it for a gear: "em=" + GearState.
fn gears_state(g: &Game) -> String {
    let l = &g.gears;
    let ids: Vec<String> = l.items[..l.size].iter().map(|x| x.unwrap_or(0).to_string()).collect();
    format!("em={},{}", l.items.len(), if ids.is_empty() { "-".into() } else { ids.join(":") })
}

pub(super) type Call = fn(&mut Game, &Case) -> Result<String, String>;

fn power_create_effect(g: &mut Game, c: &Case) -> Result<String, String> {
    c.with(0)?;
    let k = c.self_kind(crate::game::power::PowerKind::of_tool)?;
    let (_, root) = g.power_create_effect_object(k);
    Ok(format!("@{}(Clone)#{}", k.effect().rsplit('/').next().unwrap_or("").replace(' ', "_"), root + 2))
}
/// A generated free-function check: the args alone (fnpure lines).
type PureCall = fn(&Case) -> Result<String, String>;

/// The rewrite's function for each probed game method: the return value as the recorder formats it (Dump.Format),
/// "" for void.
/// A recorded struct's `key=value` word (`Stabbing(stabber=@.. point=x,y)`, `Bounds(m_Center=.. m_Extents=..)`).
fn word<'a>(arg: &'a str, key: &str) -> Result<&'a str, String> {
    let inner = arg.split_once('(').map_or(arg, |(_, r)| r.strip_suffix(')').unwrap_or(r));
    inner.split(' ').find_map(|kv| kv.strip_prefix(key)?.strip_prefix('=')).ok_or_else(|| format!("no {key} in {arg}"))
}

/// A live PinTool case's self: a WoodPinTool (its record's class) or a plain PinTool.
fn pin_wood(c: &Case) -> Result<bool, String> {
    // gapB r23: a case with no record of the tool (a GenRun case) names its type in the FnCase's selfty.
    let ty = c.s.records.iter().find(|r| r.id == c.sid && r.ty != "FnCase").map(|r| r.ty.as_str())
        .or_else(|| c.s.of("FnCase").find(|r| r.id == c.id).and_then(|r| r.get("selfty")))
        .ok_or_else(|| format!("no PinTool record #{}", c.sid))?;
    match ty { "WoodPinTool" => Ok(true), "PinTool" => Ok(false), t => Err(format!("PinTool self is a {t}")) }
}

/// A recorded vector's x and y (a Vector3's z dropped).
fn xy(text: &str) -> Result<[f32; 2], String> {
    let mut v = [0.0; 2];
    for (i, w) in text.split(',').take(2).enumerate() {
        v[i] = u32::from_str_radix(w, 16).map(f32::from_bits).map_err(|_| format!("not floats: {text}"))?;
    }
    Ok(v)
}

/// A RagdollPose argument (`RagdollPose:"Rest"`): its index among the person's Poses, by name.
fn pose_arg(g: &Game, person: usize, arg: &str) -> Result<usize, String> {
    let name = arg.strip_prefix("RagdollPose:").ok_or_else(|| format!("not a RagdollPose: {arg}"))?.trim_matches('"').replace('_', " ");
    g.people[person].Poses.iter().position(|p| p.Name == name).ok_or_else(|| format!("no pose {name}"))
}

/// gapB r5: Global.Start from BigProbeB's em= `<ActiveUiBlockers>,<pausedMenu>,<timeScaleTarget bits>,<Steam BODY_COUNT>,<local bits>,
/// <Steam TOTAL_SPAWNED_ITEMS>,<local bits>`: seeds the Global, the Steam store and the local stats; the same after as `em=`.
fn global_start_em(g: &mut Game, sid: i32, em: &str) -> Result<String, String> {
    use super::super::stats::{with_stats, BODY_COUNT, TOTAL_SPAWNED_ITEMS};
    let p: Vec<&str> = em.split(',').collect();
    let [b, pm, t, sb, lb, st, lt] = p[..] else { return Err(format!("em= not 7 fields: {em}")) };
    let bits = |v: &str| u32::from_str_radix(v, 16).map(f32::from_bits).map_err(|e| format!("{v}: {e}"));
    g.ActiveUiBlockers = b.parse().map_err(|_| format!("blockers {b}"))?;
    g.paused_menu = pm == "1";
    g.timeScaleTarget.0 = bits(t)?;
    for (stat, steam, local) in [(BODY_COUNT, sb, lb), (TOTAL_SPAWNED_ITEMS, st, lt)] {
        crate::achievements::store().seed(stat, steam.parse::<i32>().map_err(|_| format!("steam {steam}"))? as f64);
        let local = bits(local)?;
        with_stats(|s| s.set_stat(stat, local));
    }
    g.global_start(sid);
    let state = |stat: &str| format!("{},{:08x}", super::super::stats::get_int(stat), with_stats(|s| s.get_stat(stat, 0.0)).to_bits());
    Ok(format!("em={},{},{:08x},{},{}", g.ActiveUiBlockers, u8::from(g.paused_menu), g.timeScaleTarget.0.to_bits(), state(BODY_COUNT), state(TOTAL_SPAWNED_ITEMS)))
}

/// gapB r4: StatManager.IncrementInteger/IncrementFloat(stat, value). BigProbeB's em= is `<stat>,<Steam value>,<attempts>`
/// (the value as SteamUserStats.GetStatInt decimal or GetStatFloat bits, of the function's own type). The case's em= seeds
/// the rewrite's Steam store and StatManager.attempts; the ret is the same em= after the call.
fn stat_increment(c: &Case, float: bool) -> Result<String, String> {
    let w = c.with(2)?;
    let stat = arg_field(w.arg(0)?, "APIName").ok_or("no Stat APIName")?.trim_matches('"').to_string();
    let em = c.field("em").ok_or("no em=")?;
    let p: Vec<&str> = em.split(',').collect();
    let [_, v, a] = p[..] else { return Err(format!("em= not stat,value,attempts: {em}")) };
    let parse = |v: &str| -> Result<f64, String> {
        if float { u32::from_str_radix(v, 16).map(|b| f32::from_bits(b) as f64).map_err(|e| e.to_string()) } else { v.parse::<i32>().map(f64::from).map_err(|e| e.to_string()) }
    };
    crate::achievements::store().seed(&stat, parse(v)?);
    super::super::stats::check_seed(a.parse().map_err(|_| format!("attempts {a}"))?);
    if float { super::super::stats::increment_float(&stat, w.p_f32(1)?) } else { super::super::stats::increment_integer(&stat, w.p_int(1)?) }
    let after = crate::achievements::store().get(&stat, float).unwrap_or(0.0);
    let v = if float { format!("{:08x}", (after as f32).to_bits()) } else { (after as i32).to_string() };
    Ok(format!("em={stat},{v},{}", super::super::stats::check_attempts()))
}

/// gapB r3: a scene singleton's static Instance/Main accessor (BigProbeB's Singleton): the rewrite's own value, which
/// from_snapshot's scene_singletons_awake set (level1's Awake `Instance = this`); a getter returns it, a setter stores
/// its arg; the value after as the ret's em=. The case's em= (the game's value before) is only compared, as the getter's ret.
fn singleton(g: &mut Game, c: &Case) -> Result<String, String> {
    let name = &c.s.of("FnCase").next().ok_or("no FnCase")?.name;
    let (class, m) = name.split_once('.').ok_or("no class")?;
    if m.starts_with("set_") {
        let v = c.with(1)?.arg(0)?.to_string();
        g.set_singleton_instance(class, &v);
        // A void call: the recorder's ret is "em=<value>" itself.
        return Ok(format!("em={}", g.singleton_instance(class)));
    }
    c.with(0)?;
    let ret = g.singleton_instance(class);
    Ok(format!("{ret} em={}", g.singleton_instance(class)))
}

/// gapB r2: FnTiny's grid Cell arg `cell:x:y` (its key in AmbientTemperatureGridBehaviour.World).
fn cell_key(a: &str) -> Result<(i32, i32), String> {
    let (x, y) = a.strip_prefix("cell:").and_then(|v| v.split_once(':')).ok_or_else(|| format!("not a cell arg: {a}"))?;
    Ok((x.parse().map_err(|_| format!("cell x {x}"))?, y.parse().map_err(|_| format!("cell y {y}"))?))
}

/// gapA r15: a MonoBehaviour argument's GameObject as a `physicals` index (a PhysicalBehaviour, or a LimbBehaviour's).
/// gapA r16: any other component (a Circulation, a GoreString, particles with no record) means its GameObject: the
/// PhysicalBehaviour beside it on its Clone row. None: a GameObject with no PhysicalBehaviour, where no UseWireBehaviour
/// (IsolatedActivation's only receiver) can sit, so the SendMessage reaches nothing.
fn other_physical(g: &Game, c: &Case) -> Result<Option<usize>, String> {
    let id = c.args.first().and_then(|a| reference(a)).ok_or("other: null")?;
    match c.record_index(g, id) {
        Ok((t, i)) if t == "PhysicalBehaviour" => return Ok(Some(i)),
        Ok((t, i)) if t == "LimbBehaviour" => return Ok(Some(g.limbs[i].PhysicalBehaviour)),
        _ => {}
    }
    match c.sibling_id(id, "PhysicalBehaviour")? {
        Some(pid) => phys_by_id(g, pid).map(Some),
        None => Ok(None),
    }
}

/// gapC r18: a `Stab(stabber= wound= normal= point= victim=)` arg as the rewrite's Stabbing; the victim is limb `l`'s physical.
fn stab_arg(g: &Game, c: &Case, l: usize) -> Result<super::super::stab::Stabbing, String> {
    let stabber = phys_by_id(g, c.rec::<i32>("Stab(", "stabber")?)?;
    Ok(super::super::stab::Stabbing { stabber, victim: g.limbs[l].PhysicalBehaviour, normal: c.rec("Stab(", "normal")?, point: c.rec("Stab(", "point")? })
}
/// MapConfigProbe's wound=0 case turned the stabber's StabCausesWound off; ListenPost turns it back on before the PO snapshot.
fn stab_probe_reset(g: &mut Game, c: &Case) -> Result<(), String> {
    if c.rec::<i32>("Stab(", "wound")? == 0 {
        let s = phys_by_id(g, c.rec::<i32>("Stab(", "stabber")?)?;
        g.physicals[s].StabCausesWound = true;
    }
    Ok(())
}
/// gapC r18: an `ExplosionParameters(FragmentationRayCount= Position= Range= FragmentForce= CreateParticlesAndSound= EffectSize=
/// DismemberChance= BallisticShrapnelCount=)` arg (EffectSize: the enum's Medium 0, Small 1, Large 2).
fn explosion_arg(a: &str) -> Result<super::super::explosion::ExplosionParameters, String> {
    use super::super::explosion::EffectSize as S;
    let get = |k: &str| arg_field(a, k).ok_or_else(|| format!("ExplosionParameters without {k}"));
    let int = |k: &str| -> Result<i64, String> { get(k)?.parse().map_err(|_| format!("ExplosionParameters {k}")) };
    Ok(super::super::explosion::ExplosionParameters {
        FragmentationRayCount: int("FragmentationRayCount")? as u32,
        Position: floats::<3>(get("Position")?),
        Range: item_f32(get("Range")?)?,
        FragmentForce: item_f32(get("FragmentForce")?)?,
        CreateParticlesAndSound: get("CreateParticlesAndSound")? == "1",
        EffectSize: match int("EffectSize")? { 0 => S::Medium, 1 => S::Small, 2 => S::Large, x => return Err(format!("EffectSize {x}")) },
        DismemberChance: item_f32(get("DismemberChance")?)?,
        BallisticShrapnelCount: int("BallisticShrapnelCount")? as i32,
    })
}

const CALLS: &[(&str, Call)] = &[
    // fnrec r2: game signatures the GEN entries paired wrong. Instance getters/methods on the self: the self's state.
    // CanTransferBlood: containerA and containerB alive and both AllowsTransfer.
    ("BloodWireBehaviour.get_CanTransferBlood", |g, c| {
        c.with(0)?;
        let w = super::fnweap::wire_index(g, c.sid).ok_or(format!("no wire #{}", c.sid))?;
        let (a, b) = (g.blood_wires[w].containerA.filter(|&o| g.wire_container_alive(o)), g.blood_wires[w].containerB.filter(|&o| g.wire_container_alive(o)));
        Ok(bit(matches!((a, b), (Some(a), Some(b)) if g.blood_wire_can_transfer(a, b))))
    }),
    // GetReferenceAngle(): the wire's body against otherPhysicalBehaviour.
    ("FixedJointWireBehaviour.GetReferenceAngle", |g, c| {
        c.with(0)?;
        let w = super::fnweap::wire_index(g, c.sid).ok_or(format!("no wire #{}", c.sid))?;
        let o = g.blood_wires[w].other.ok_or("no otherPhysicalBehaviour")?;
        Ok(Enc::enc(g.fixed_wire_get_reference_angle(g.blood_wires[w].physical, o)))
    }),
    // IsNearOtherDecal(localPosition): the controller from em= (its physical's id, index), IgnoreRadius of its descriptor.
    ("DecalControllerBehaviour.IsNearOtherDecal", |g, c| {
        let w = c.with(2)?;
        let (pid, k) = c.field("em").ok_or("no em=")?.split_once(',').ok_or("bad em=")?;
        let pid: i32 = pid.parse().map_err(|_| "em physical")?;
        let k: usize = k.parse().map_err(|_| "em index")?;
        let p = g.physicals.iter().position(|x| x.id == pid).ok_or("no physical of the controller")?;
        let dc = g.physicals[p].decal_controllers.get(k).ok_or(format!("no decal controller {k}"))?;
        let r = g.decal_descriptors[dc.descriptor].IgnoreRadius;
        Ok(Enc::enc(g.decal_is_near_other_decal(p, k, r, w.p_arr::<2>(1)?)))
    }),
    // GetNearEntry(localPos, minDistance, type, out index): ret and the out args (index = currentDamagePointCount on a miss).
    ("SkinMaterialHandler.GetNearEntry", |g, c| {
        let w = c.with_self(4)?;
        let l = w.p_usize(g, 0)?;
        let found = g.skin_get_near_entry(l, w.p_arr::<3>(1)?, w.p_f32(2)?, w.p_int(3)?);
        let index = found.unwrap_or(g.skins[l].currentDamagePointCount as usize);
        Ok(format!("{} out={};{};{};{index}", bit(found.is_some()), c.args[0], c.args[1], c.args[2]))
    }),
    // Use(activation): the signal arg is not read; the self.
    // fnrec r3: isFiring from the recorder's " stpre=rg:" (private, no record holds it), the call, then " st=rg:" (FireCoroutine sets it
    // before its first yield).
    ("RailgunBehaviour.Use", |g, c| {
        let w = c.with_self(1)?;
        let i = w.p_usize(g, 0)?;
        let pre = st_pre(c).and_then(|p| p.strip_prefix("rg:"));
        if let Some(p) = pre { g.railguns[i].isFiring = p == "1"; }
        g.railgun_shoot(i);
        Ok(pre.map(|_| format!(" st=rg:{}", bit(g.railguns[i].isFiring))).unwrap_or_default())
    }),
    ("TruckDoorBehaviour.Use", |g, c| { let w = c.with_self(1)?; let i = w.p_usize(g, 0)?; g.truck_door_use(i); VOID }),
    // SendContinuous(activation, target): target.OnContinuousUse has no listener (activations_send_continuous).
    // A bare "signal" arg is dropped from the args (Case), so the target is the last.
    ("Activations.SendContinuous", |g, c| { let n = c.args.len(); let w = c.with(n)?; let p = w.p_usize(g, n.checked_sub(1).ok_or("no target arg")?)?; g.activations_send_continuous(p); VOID }),
    // fnrec r2: the self by its id (the ItemScript record order is not the arena's).
    ("BreakJointOnBurnBehaviour.Awake", |g, c| { c.with(1)?; let b = g.break_joint_on_burns.iter().position(|x| x.id == c.sid).ok_or(format!("no BreakJointOnBurn #{}", c.sid))?; g.break_joint_on_burn_awake(b); VOID }),
    // gapC r27: PoolGenerator.RequestPrefab(originalPrefab, position): the arg `@<prefab>#id` by name among the prefabs the
    // game requests (the people's PoolableImpactEffect, the properties' ShotImpact); ret the clone's GameObject as the recorder
    // writes it: a new one takes the next id after Ensure's three (a new pool's GameObject, Transform and ObjectPoolBehaviour).
    ("PoolGenerator.RequestPrefab", |g, c| {
        let c = c.with(2)?;
        let pos = c.p_arr::<2>(1)?;
        let name = c.arg(0)?.strip_prefix('@').and_then(|a| a.split('#').next()).ok_or("prefab arg is no reference")?.to_string();
        for i in 0..g.properties.len() { if matches!(g.properties[i].ShotImpact, Some((None, _))) { g.recorded_shot_impact(i) } }
        let mut db = g.assets.take().ok_or("no game data")?;
        let cands: Vec<_> = g.people.iter().filter_map(|p| p.PoolableImpactEffect.flatten()).chain(g.properties.iter().filter_map(|p| p.ShotImpact.and_then(|s| s.0))).collect();
        let prefab = cands.into_iter().find(|&r| db.prefab(r).is_ok_and(|p| p.objects[0].name.replace(' ', "_") == name));
        g.assets = Some(db);
        let prefab = prefab.ok_or_else(|| format!("{name}: no prefab the game requests"))?;
        let fresh = g.pool_named(&format!("{name}_pool")).is_none();
        let (before, n0) = (g.blood_puffs.len() + g.splash_pools.iter().map(|p| p.items.len()).sum::<usize>(), g.next_instance_id);
        if !g.pool_generator_request_prefab(prefab, pos) { return Ok("null".into()) }
        // A reused clone keeps the id of an earlier case (not tracked here).
        if g.blood_puffs.len() + g.splash_pools.iter().map(|p| p.items.len()).sum::<usize>() == before { return Ok(format!("@{name}(Clone)#reused")) }
        Ok(format!("@{name}(Clone)#{}", n0 - if fresh { 6 } else { 0 }))
    }),
    // gapC r28: private state no snapshot holds, loaded from the FnCase's " stpre=" and given back as " st=" (Recorder.cs
    // FnTiny.StateOf): DiskLogger dl:<writer open>,<listener on>; UISoundBehaviour uis:<Main is self>,<bypassListenerEffects>;
    // SortingLayerChild slc:<parentRenderer set>.
    ("DiskLogger.OnDestroy", |g, c| {
        c.with(0)?;
        match st_pre(c) {
            Some("dl:1,1") => {}
            Some("dl:0,0") => g.disk_logger = None,
            p => return Err(format!("DiskLogger stpre {p:?}: no rewrite state")),
        }
        g.disk_logger_on_destroy();
        Ok(format!(" st={}", g.disk_logger_state()))
    }),
    // fc41 r2: PersonBehaviour.OnDestroy: RemoveFromDictionary, st=pal:<AliveByTransform holds it> (the recorder puts the
    // entry back before each case).
    ("PersonBehaviour.OnDestroy", |g, c| {
        let p = c.with(1)?.p_usize(g, 0)?;
        if st_pre(c) != Some("pal:1") { return Err(format!("PersonBehaviour.OnDestroy stpre {:?}", st_pre(c))) }
        g.people[p].alive_entry = true;
        g.person_on_destroy(p);
        Ok(format!(" st=pal:{}", g.people[p].alive_entry as u8))
    }),
    // fc41 r2: BloodTankBehaviour.OnDestroy: OnCollide -= OnParticleCollide (lis=<before>,<after>, MapConfigProbe GenListeners).
    ("BloodTankBehaviour.OnDestroy", |g, c| {
        let i = c.with(1)?.p_usize(g, 0)?;
        let pre = g.blood_tank_listeners(i);
        g.blood_tank_on_destroy(i);
        Ok(format!("lis={pre},{}", g.blood_tank_listeners(i)))
    }),
    ("UISoundBehaviour.Start", |g, c| {
        c.with(0)?;
        let this = g.ui_sound.take().ok_or("no UISoundBehaviour.Main in ours")?;
        let pre = st_pre(c).and_then(|p| p.strip_prefix("uis:")).ok_or("no uis stpre")?;
        g.audio.sources.get_mut(&this.source).ok_or("UISoundBehaviour.AudioSource")?.bypass_listener_effects = pre.ends_with(",1");
        g.ui_sound = pre.starts_with("1,").then(|| this.clone());
        super::super::scene::ui_sound_behaviour_start(&mut g.audio, &mut g.ui_sound, this)?;
        let by = g.ui_sound.as_ref().and_then(|u| g.audio.sources.get(&u.source)).is_some_and(|s| s.bypass_listener_effects);
        Ok(format!(" st=uis:{},{}", g.ui_sound.is_some() as u8, by as u8))
    }),
    ("SortingLayerChild.OnTransformParentChanged", |g, c| {
        use super::super::visual::Kind;
        c.with(0)?;
        let v = g.visuals.iter().position(|x| x.id == c.sid).ok_or_else(|| format!("no SortingLayerChild #{}", c.sid))?;
        if st_pre(c) == Some("slc:0") {
            if let Kind::SortingChild { parent, .. } = &mut g.visuals[v].kind {
                *parent = None;
            }
        }
        g.with_component_instance(c.sid, |g, inst, o| g.sorting_layer_child_parent_changed(inst, o));
        Ok(format!(" st=slc:{}", matches!(g.visuals[v].kind, Kind::SortingChild { parent: Some(_), .. }) as u8))
    }),
    // gapC r27: ExistInDetailView.Main_LimbStatusToggled on the case's own ExistInDetailView (its object's SetActive(e)).
    ("ExistInDetailView.Main_LimbStatusToggled", |g, c| { let e = c.with(1)?.p_bool(0)?; g.exist_in_detail_view_toggled(c.sid, e); VOID }),
    // gapC r16: generated entries the map no longer generates (the liquids' empty-body OnExitContainer / OnEnterContainer
    // rows, BeverageM04): kept here so a gen_fncheck.py regen does not drop them.
    ("AdrenalineLiquid.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("BeverageM04.OnEnterContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.chemistry_liquid_on_enter_container(a0, a1); VOID }),
    ("BeverageM04.OnEnterLimb", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_usize(g, 1)?; g.chemistry_liquid_on_enter_limb(a0, a1); VOID }),
    ("BeverageM04.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.chemistry_liquid_on_exit_container(a0, a1); VOID }),
    ("BoneHurtingJuiceLiquid.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("CirculationPoison.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("CoagulationLiquid.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("CombustionAgent.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("CrushingPoison.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("DurabilitySerum.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("ExplosionPoison.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("FreezePoisonLiquid.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("ImmortalitySerum.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("InstantDeathPoisonLiquid.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("KnockoutPoisonLiquid.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("LifeSerumLiquid.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("MassManipulationPoison.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("MendingSerum.OnEnterContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_enter_container(a0, a1); VOID }),
    ("MendingSerum.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("MuscleEnhancementSerum.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("MusclePoison.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("NumbingPoison.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("OsteomorphosisAgent.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("PainKillerLiquid.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("PinkDormantLiquid.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("ReflectionPoison.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("RegenerationSerum.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("SizeManipulationPoison.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("TissueDeconstructionAgent.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("TransparencyPoison.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("UltraStrengthSerumLiquid.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("VestibularPoison.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("WaterBreathingSerum.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    ("ZombiePoisonLiquid.OnEnterContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_enter_container(a0, a1); VOID }),
    ("ZombiePoisonLiquid.OnExitContainer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.syringe_liquid_on_exit_container(a0, a1); VOID }),
    // gapB r15: the self's own OnJointBreak2D (GEN's break_joint_scripts_on_break destroyed its sibling script too).
    ("BreakJointOnBurnBehaviour.OnJointBreak2D", |g, c| { let c = c.with(1)?; let a0 = c.p_int(0)?; g.break_joint_script_on_joint_break(c.sid(), a0); VOID }),
    ("BreakJointOnShotBehaviour.OnJointBreak2D", |g, c| { let c = c.with(1)?; let a0 = c.p_int(0)?; g.break_joint_script_on_joint_break(c.sid(), a0); VOID }),
    // gapB r3: the scene singletons' static accessors (Game::statics, set by the rewrite's scene_singletons_awake).
    ("BulletTracerPool.get_Instance", singleton), ("BulletTracerPool.set_Instance", singleton),
    ("ClipboardControllerBehaviour.get_Main", singleton), ("ClipboardControllerBehaviour.set_Main", singleton),
    ("ContinuousActivationBehaviour.get_Instance", singleton), ("ContinuousActivationBehaviour.set_Instance", singleton),
    ("GlobalShrapnelEmitter.get_Instance", singleton), ("GlobalShrapnelEmitter.set_Instance", singleton),
    ("LimbSpriteCache.get_Instance", singleton), ("LimbSpriteCache.set_Instance", singleton),
    ("PoolGenerator.get_Instance", singleton), ("PoolGenerator.set_Instance", singleton),
    ("ProceduralShatterMaterialPool.get_Instance", singleton), ("ProceduralShatterMaterialPool.set_Instance", singleton),
    ("ThermalVisionBehaviour.get_Instance", singleton), ("ThermalVisionBehaviour.set_Instance", singleton),
    ("ToolLibrary.get_Instance", singleton), ("ToolLibrary.set_Instance", singleton),
    ("ToolWheelBehaviour.get_Instance", singleton), ("ToolWheelBehaviour.set_Instance", singleton),
    ("PianoDialogBox.get_Instance", singleton), ("PianoDialogBox.set_Instance", singleton),
    ("BackgroundItemLoader.get_Instance", singleton), ("BackgroundItemLoader.set_Instance", singleton),
    ("BackgroundItemLoaderStatusBehaviour.get_Instance", singleton), ("BackgroundItemLoaderStatusBehaviour.set_Instance", singleton),
    ("LayerEditorController.get_Instance", singleton), ("LayerEditorController.set_Instance", singleton),
    // gapB r3: JavelinBehaviour.TransferTarget(source, projectile): the projectile GameObject's JavelinMissileBehaviour (its
    // Clone record's comps), none for another GameObject (a human limb: the LogWarning return).
    ("JavelinBehaviour.TransferTarget", |g, c| { let w = c.with(2)?; let j = g.javelins.iter().position(|x| x.id == c.id).ok_or("no javelins entry")?;
        let go = reference(w.arg(1)?).ok_or("null projectile")?;
        let mid = c.s.of("Clone").find(|r| r.id == go).and_then(|r| r.get("comps")).and_then(|x| x.trim_matches(['[', ']']).split(';').find_map(|e| e.strip_prefix("JavelinMissileBehaviour:")?.parse::<i32>().ok()));
        let m = match mid { Some(id) => Some(g.javelin_missiles.iter().position(|x| x.id == id).ok_or("no javelin_missiles entry")?), None => None };
        g.javelin_transfer_target(j, m); VOID }),
    // gapB r2: Activations.SendOnce(a, target): target.OnSingleUse.Invoke(a) (the gen twin passed the signal as the target).
    // The recorder's plain signal (channel 0, direct) is dropped from the args: the target is the last one.
    ("Activations.SendOnce", |g, c| { let n = c.args.len(); let t = c.with(n)?.p_usize(g, n - 1)?; g.activations_send_once(t); VOID }),
    // fc41 r6: BalloonBehaviour.Shot starts WaitAFrame: " st=bw:<its wait>" (FnTiny.R4Of); a " balpop=1" case runs it on to Pop.
    // fc41 r8: SoapBehaviour.Clean; a seeded case (" scs=1") has the hit controller's decal alphas after (FnTiny StateOf st=sc:).
    // fc41 r9: the dump holds no decal colour: the case's " sca=" (the alphas FnTiny set and read back before the call) is that
    // state, put on the controller's decals in order before the call.
    ("SoapBehaviour.Clean", |g, c| {
        let w = c.with(2)?;
        let (s, hit) = (w.p_usize(g, 0)?, w.p_int(1)?);
        let case = c.s.of("FnCase").next();
        if case.and_then(|r| r.get("scs")) != Some("1") { g.soap_clean(s, hit); return VOID }
        let q = *g.soap_decal_physicals(hit).first().ok_or("scs case: no decal-holding physical on the hit")?;
        if g.physicals[q].decal_controllers.len() != 1 { return Err(format!("scs case: physical #{} has {} decal controllers", g.physicals[q].id, g.physicals[q].decal_controllers.len())) }
        let sca: Vec<f32> = case.and_then(|r| r.get("sca")).ok_or("scs case: no sca")?.split(',').filter(|v| !v.is_empty()).map(|v| u32::from_str_radix(v, 16).map(f32::from_bits).map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
        let decals = &mut g.physicals[q].decal_controllers[0].decals;
        if decals.len() != sca.len() { return Err(format!("scs case: {} decals, sca has {}", decals.len(), sca.len())) }
        for (d, a) in decals.iter_mut().zip(sca) { d.colour[3] = a; }
        g.soap_clean(s, hit);
        Ok(format!(" st=sc:{}", g.physicals[q].decal_controllers[0].decals.iter().map(|d| format!("{:08x}", d.colour[3].to_bits())).collect::<Vec<_>>().join(",")))
    }),
    // fc41 r9: PoisonSpreadBehaviour.OnPoisonOther is an empty virtual (no override in Assembly-CSharp); the generated entry
    // called poison_poison (Poison, its caller).
    ("PoisonSpreadBehaviour.OnPoisonOther", |_, _| VOID),
    // fc41 r10: SyringeBehaviour.Awake on a " sya=1" case: FnTiny put a block of other values in materialProperty and
    // wrote the renderer's block (not in the dump) as " syb="; the return is materialProperty after (st=sa:, FnTiny.SyBlock).
    ("SyringeBehaviour.Awake", |g, c| {
        let i = g.syringes.iter().position(|x| x.id == c.id).ok_or("no syringes entry")?;
        let case = c.s.of("FnCase").next();
        if case.and_then(|r| r.get("sya")) != Some("1") { g.syringe_awake(i); return VOID }
        let f = |v: &str| u32::from_str_radix(v, 16).map(f32::from_bits).map_err(|e| e.to_string());
        let syb: Vec<&str> = case.and_then(|r| r.get("syb")).ok_or("sya case: no syb")?.split(',').collect();
        let [empty, r, gr, b, a, dir] = syb[..] else { return Err(format!("syb: {} values", syb.len())) };
        let mut rb = crate::game::props::PropBlock::default();
        if empty == "0" { rb.set_color("_LiquidColour", [f(r)?, f(gr)?, f(b)?, f(a)?]); rb.set_float("_Direction", f(dir)?); }
        let at = crate::game::visual::SpriteAt::Physical(g.syringes[i].Phys);
        // fc41 r11: the renderer's block as loaded from PI (exact linear vectors) goes back after the call: Awake only reads it,
        // and the gamma " syb=" colour set here turns back into linear a few ulp off (GetColor's LinearToGamma is lossy).
        let loaded = g.get_property_block(at);
        g.set_property_block(at, &rb);
        let mut junk = crate::game::props::PropBlock::default();
        junk.set_color("_LiquidColour", [0.25, 0.5, 0.75, 0.125]);
        junk.set_float("_Direction", 7.0);
        g.syringes[i].materialProperty = junk;
        g.syringe_awake(i);
        g.set_property_block(at, &loaded);
        let mp = &g.syringes[i].materialProperty;
        let lc = match mp.get("_LiquidColour") { Some(crate::game::props::Prop::Color(v)) => *v, _ => [0.0; 4] };
        let h = |x: f32| format!("{:08x}", x.to_bits());
        Ok(format!(" st=sa:{},{},{}", mp.0.is_empty() as u8, lc.map(h).join(","), h(mp.get_float("_Direction"))))
    }),
    // fc41 r15: SingleFloodlightBehaviour.Awake on a " fla=1" case: FnTiny gave the Renderer's and each Highlight sprite's block a
    // _GlowIntensity (" flr=", " flh=": not in the dump) and junk in the held blocks; the return is the held blocks after (st=fl:).
    // fc41 r16: Use(ActivationPropagation): its Channel. Older recordings passed the channel as an int; a bare "signal"
    // (Direct, channel 0) is dropped from the args.
    ("ValveBehaviour.Use", |g, c| {
        let i = g.valves.iter().position(|x| x.id == c.id).ok_or("no valves entry")?;
        let ch = match c.args.first() { Some(a) if !a.starts_with("signal") => c.arg(0)?, a => signal_arg(a.copied().unwrap_or("signal"))?.1 as i32 };
        g.valve_use(i, ch);
        VOID
    }),
    // fc41 r16: the FnTiny seed (emn: the nodes; the mesh lists cleared); st=em:<enabled>,<vertices>,<colours>,<indices>,<FNV of the bits>.
    // fc41 r18: the case's fuzzed fields (its ItemScript record): the private cursorStartPos, barStartPos and barSize and
    // CursorMaxDistance, ThresholdPercentage, MaxPower and enabled, which the gate loaded from another case keeps.
    // fc41 r19: the line tool's OnSelect as the self's own kind (selfty=), which `chosen`/`created` are kept for.
    ("SpringJointWireTool.OnSelect", wire_on_select_as_self),
    ("FixedWireTool.OnSelect", wire_on_select_as_self),
    ("GateBehaviour.OnWillRenderObject", |g, c| {
        c.with(0)?;
        let i = g.gates.iter().position(|x| x.id == c.sid).ok_or("no gates entry")?;
        let id = c.sid.to_string();
        let rec = c.s.of("ItemScript").find(|r| r.id.to_string() == id && r.get("class") == Some("GateBehaviour")).ok_or("no GateBehaviour record")?;
        let hex = |v: &str| u32::from_str_radix(v, 16).map(f32::from_bits).map_err(|e| e.to_string());
        let one = |k: &str| rec.req(k).and_then(hex);
        let three = |k: &str| -> Result<[f32; 3], String> {
            let v: Vec<f32> = rec.req(k)?.split(',').map(hex).collect::<Result<_, _>>()?;
            v.try_into().map_err(|_| format!("{k}: not 3 values"))
        };
        let x = &mut g.gates[i];
        (x.CursorMaxDistance, x.ThresholdPercentage, x.MaxPower) = (one("CursorMaxDistance")?, one("ThresholdPercentage")?, one("MaxPower")?);
        (x.cursorStartPos, x.barStartPos, x.barSize) = (three("cursorStartPos")?, three("barStartPos")?, three("barSize")?);
        // The fuzzed Behaviour.enabled (OnWillRenderObject's `if (base.enabled)`).
        x.enabled = rec.req("enabled")? == "1";
        g.gates_will_render();
        VOID
    }),
    // fc41 r18: the case's fuzzed private `time` (its ItemScript record), which the loaded explosion keeps as a link.
    ("SeismicChargeExplosionBehaviour.FixedUpdate", |g, c| {
        let i = g.seismic_explosions.iter().position(|x| x.id == c.id).ok_or("no seismic_explosions entry")?;
        let id = c.id.to_string();
        let t = c.s.of("ItemScript").find(|r| r.id.to_string() == id && r.get("class") == Some("SeismicChargeExplosionBehaviour")).and_then(|r| r.get("time")).and_then(|x| u32::from_str_radix(x, 16).ok()).map(f32::from_bits);
        if let Some(t) = t { g.seismic_explosions[i].time = t }
        // " mag=" (recordings from fc41 r18 on): Magnet.enabled at the call, which no snapshot record holds (an earlier
        // case's FixedUpdate turned it off).
        if let Some(m) = c.s.of("FnCase").next().and_then(|r| r.get("mag")) {
            let magnet = g.seismic_explosions[i].Magnet;
            g.effector_set_enabled(magnet, m == "1");
        }
        g.seismic_explosion_fixed_update(i);
        Ok(String::new())
    }),
    // fc41 r17: the case's fuzzed JitterIntensity and offset (its ItemScript record), which the visual's loaded kind lacks.
    ("HologramJitter.OnWillRenderObject", |g, c| {
        let v = { let c = c.with(1)?; c.p_visual(g, 0)? };
        let id = g.visuals[v].id.to_string();
        let rec = c.s.of("ItemScript").find(|r| r.id.to_string() == id && r.get("class") == Some("HologramJitter"));
        let hex = |k: &str| rec.and_then(|r| r.get(k)).and_then(|x| u32::from_str_radix(x, 16).ok()).map(f32::from_bits);
        if let crate::game::visual::Kind::Hologram { JitterIntensity, offset, .. } = &mut g.visuals[v].kind {
            if let Some(x) = hex("JitterIntensity") { *JitterIntensity = x }
            if let Some(x) = hex("offset") { *offset = x }
        }
        g.hologram_will_render(v);
        VOID
    }),
    ("EnergySwordMotionBlur.UpdateMesh", |g, c| {
        let i = { let c = c.with(1)?; c.p_usize(g, 0)? };
        let case = c.s.of("FnCase").next();
        let Some(emn) = case.and_then(|r| r.get("emn")) else { g.energy_blur_update_mesh(i); return VOID };
        let mut nodes = Vec::new();
        for (k, n) in emn.split(';').filter(|x| *x != "-").enumerate() {
            let v: Vec<f32> = n.split(',').map(|v| u32::from_str_radix(v, 16).map(f32::from_bits).map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
            if v.len() != 6 { return Err(format!("emn node {k}: {} values", v.len())) }
            nodes.push(crate::game::energy_sword::BlurNode { Life: 0.1 * k as f32, A: [v[0], v[1], v[2]], B: [v[3], v[4], v[5]] });
        }
        let old = std::mem::replace(&mut g.energy_blurs[i].nodes, nodes);
        // The case's fuzzed MakeBottomColorTransparent (its ItemScript record at the call), which the loaded blur lacks.
        let me = case.and_then(|r| r.get("self")).map(|s| s.to_string());
        let mb = c.s.of("ItemScript").find(|r| Some(r.id.to_string()) == me && r.get("class") == Some("EnergySwordMotionBlur")).and_then(|r| r.get("MakeBottomColorTransparent")).map(|v| v == "1");
        let old_mb = g.energy_blurs[i].MakeBottomColorTransparent;
        if let Some(m) = mb { g.energy_blurs[i].MakeBottomColorTransparent = m }
        { let b = &mut g.energy_blurs[i]; b.vertices.clear(); b.colors.clear(); b.indices.clear(); }
        g.energy_blur_update_mesh(i);
        let b = &mut g.energy_blurs[i];
        b.nodes = old;
        b.MakeBottomColorTransparent = old_mb;
        let fnv = |xs: &mut dyn Iterator<Item = u32>| xs.fold(2166136261u32, |h, x| (h ^ x).wrapping_mul(16777619));
        let hv = fnv(&mut b.vertices.iter().flatten().map(|f| f.to_bits()));
        let hc = fnv(&mut b.colors.iter().flatten().map(|f| f.to_bits()));
        let hi = fnv(&mut b.indices.iter().copied());
        let (n, x) = (b.vertices.len(), |f: f32| format!("{:08x}", f.to_bits()));
        let tail = if n >= 2 { let (v, w, q) = (b.vertices[n - 2], b.vertices[n - 1], b.colors[b.colors.len() - 2]); format!(",{},{},{},{},{},{}", x(v[0]), x(v[1]), x(w[0]), x(w[1]), x(q[0]), x(q[3])) } else { String::new() };
        Ok(format!(" st=em:{},{n},{},{},{hv:08x},{hc:08x},{hi:08x}{tail}", b.renderer_enabled as u8, b.colors.len(), b.indices.len()))
    }),
    // fc41 r16: the FnTiny seed (rxt/rxc, the core's InternalTemperature and CoolingPower; text "junk");
    // st=rx:<length>,<FNV-1a of the UTF-16 code units of TextMesh.text>.
    ("ReactorA5DisplayBehaviour.UpdateDisplayText", |g, c| {
        let i = { let c = c.with(1)?; c.p_usize(g, 0)? };
        let case = c.s.of("FnCase").next();
        let f = |k: &str| case.and_then(|r| r.get(k)).map(|v| u32::from_str_radix(v, 16).map(f32::from_bits).map_err(|e| e.to_string()));
        let (Some(t), Some(cp)) = (f("rxt"), f("rxc")) else { g.reactor_a5_display_update_text(i); return VOID };
        let core = g.reactor_a5_displays[i].Core.ok_or("rx case: no Core")?;
        let old = (g.reactor_cores[core].InternalTemperature, g.reactor_cores[core].CoolingPower);
        (g.reactor_cores[core].InternalTemperature, g.reactor_cores[core].CoolingPower) = (t?, cp?);
        let text = g.reactor_a5_displays[i].TextMesh;
        if let Some(x) = text { g.text_set(x, "junk") }
        g.reactor_a5_display_update_text(i);
        (g.reactor_cores[core].InternalTemperature, g.reactor_cores[core].CoolingPower) = old;
        let Some(x) = text else { return Ok(" st=rx:n".into()) };
        let s = g.text_get(x);
        let h = s.encode_utf16().fold(2166136261u32, |h, ch| (h ^ ch as u32).wrapping_mul(16777619));
        Ok(format!(" st=rx:{},{h:08x}", s.encode_utf16().count()))
    }),
    // fc41 r16: the FnTiny seed (tve/tvt and the transform the dump lacks); st=tv:<enabled>,<t bits>,<heat sum>,<alpha sum>,<fnv of data>.
    ("ThermalVisionBehaviour.Update", |g, c| {
        let case = c.s.of("FnCase").next();
        let Some(tve) = case.and_then(|r| r.get("tve")) else { g.thermal_vision_update(); return VOID };
        let f = |k: &str| -> Result<Vec<f32>, String> { case.and_then(|r| r.get(k)).ok_or(format!("tv case: no {k}"))?.split(',').map(|v| u32::from_str_radix(v, 16).map(f32::from_bits).map_err(|e| e.to_string())).collect() };
        let (t, p, s, r) = (f("tvt")?[0], f("tvp")?, f("tvs")?, f("tvr")?[0]);
        if r != 0.0 { return Err(format!("tv case: rotation {r}, ours reads TransformPoint without it")) }
        let tvm = f("tvm");
        let tvl = case.and_then(|r| r.get("tvl")).map(|v| v.parse::<i32>().map(|x| x as u32).map_err(|e| e.to_string())).transpose()?;
        let old_en = g.thermal_vision.replace(tve == "1");
        let old_amb = (g.AmbientTemperature, g.prefs.AmbientTemperatureTransfer);
        if let Ok(v) = f("tva") { g.AmbientTemperature = v[0] }
        if let Some(x) = case.and_then(|r| r.get("tvx")) { g.prefs.AmbientTemperatureTransfer = x == "1" }
        let old = {
            let x = g.thermal_texture.as_mut().ok_or("no thermal texture")?;
            let old = (x.t, x.data.clone(), x.position, x.scale, x.visible, x.min_temp, x.max_temp, x.to_read);
            if let Ok(m) = &tvm { (x.min_temp, x.max_temp) = (m[0], m[1]) }
            if let Some(l) = tvl { x.to_read = l }
            x.t = t; x.data.iter_mut().for_each(|b| *b = 0); x.position = [p[0], p[1]]; x.scale = [s[0], s[1]];
            // FitCameraBackgroundBehaviour.Update's camera follow (ours runs it here) is not this function's.
            x.visible = true;
            old
        };
        g.thermal_vision_update();
        let x = g.thermal_texture.as_mut().ok_or("no thermal texture after")?;
        let (mut h, mut sh, mut sa) = (2166136261u32, 0u64, 0u64);
        for (i, &b) in x.data.iter().enumerate() { h = (h ^ b as u32).wrapping_mul(16777619); if i % 2 == 0 { sh += b as u64 } else { sa += b as u64 } }
        let st = format!(" st=tv:{},{:08x},{sh},{sa},{h:08x},{:08x},{:08x},{:08x},{}", (g.thermal_vision == Some(true)) as u8, x.t.to_bits(), x.min_temp.to_bits(), x.max_temp.to_bits(), g.AmbientTemperature.to_bits(), g.prefs.AmbientTemperatureTransfer as u8);
        (x.t, x.data, x.position, x.scale, x.visible, x.min_temp, x.max_temp, x.to_read) = old;
        g.thermal_vision = old_en;
        (g.AmbientTemperature, g.prefs.AmbientTemperatureTransfer) = old_amb;
        Ok(st)
    }),
    ("SingleFloodlightBehaviour.Awake", |g, c| {
        use crate::game::props::PropBlock;
        let i = g.floodlights.iter().position(|x| x.id == c.id).ok_or("no floodlights entry")?;
        let case = c.s.of("FnCase").next();
        if case.and_then(|r| r.get("fla")) != Some("1") { g.floodlight_awake(i); return VOID }
        let f = |v: &str| u32::from_str_radix(v, 16).map(f32::from_bits).map_err(|e| e.to_string());
        let flr = case.and_then(|r| r.get("flr")).ok_or("fla case: no flr")?;
        let flh: Vec<&str> = case.and_then(|r| r.get("flh")).unwrap_or("").split(';').filter(|x| !x.is_empty()).collect();
        if flh.len() != g.floodlights[i].Highlights.len() { return Err(format!("flh: {} values, {} highlights", flh.len(), g.floodlights[i].Highlights.len())) }
        let mut put = Vec::new();
        let mut seed = |g: &mut Game, at, v: &str| -> Result<(), String> {
            if v == "n" { return Ok(()) }
            let old = g.get_property_block(at);
            let mut b = old.clone();
            b.set_float("_GlowIntensity", f(v)?);
            g.set_property_block(at, &b);
            put.push((at, old));
            Ok(())
        };
        if let Some(r) = g.floodlights[i].Renderer { seed(g, r, flr)? }
        for (h, v) in flh.iter().enumerate() { let at = g.floodlights[i].Highlights[h].SpriteRenderer; seed(g, at, v)? }
        let junk = |v: f32| { let mut b = PropBlock::default(); b.set_float("_GlowIntensity", v); b };
        g.floodlights[i].propertyBlock = junk(7.0);
        for h in &mut g.floodlights[i].Highlights { h.PropertyBlock = junk(9.0) }
        g.floodlight_awake(i);
        for (at, old) in put.into_iter().rev() { g.set_property_block(at, &old) }
        let one = |b: &PropBlock| format!("{},{:08x}", b.0.is_empty() as u8, b.get_float("_GlowIntensity").to_bits());
        let s = &g.floodlights[i];
        let first = if s.Renderer.is_some() { one(&s.propertyBlock) } else { "n".into() };
        Ok(format!(" st=fl:{}", std::iter::once(first).chain(s.Highlights.iter().map(|h| one(&h.PropertyBlock))).collect::<Vec<_>>().join("/")))
    }),
    // fc41 r13: SerialisableIdentity.Regenerate: Guid.NewGuid can't be reproduced, so FnTiny's st=sid: is <changed>,<not empty>.
    ("SerialisableIdentity.Regenerate", |g, c| {
        let c = c.with(1)?;
        let tid = c.p_int(0)?;
        let pre = g.identities.get(&tid).copied();
        g.identity_regenerate(tid);
        let post = g.identities.get(&tid).copied();
        Ok(format!(" st=sid:{},{}", (post != pre) as u8, post.is_some_and(|x| x != 0) as u8))
    }),
    // fc41 r12: DamagableMachineryBehaviour.Stabbed: the generated entry read the Stab arg as an index. The self by id, the
    // Stab's stabber; MapConfigProbe's wound=0 reset after (stab_probe_reset).
    ("DamagableMachineryBehaviour.Stabbed", |g, c| {
        let m = g.machinery.iter().position(|x| x.id == c.id).ok_or("no machinery entry")?;
        let s = phys_by_id(g, c.rec::<i32>("Stab(", "stabber")?)?;
        g.machinery_stabbed(m, s);
        stab_probe_reset(g, c)?;
        VOID
    }),
    // fc41 r11: DecalControllerBehaviour.Clear (an instance method; the generated entry cleared every controller): the controller
    // from em=, ret as BigProbeB's "dc=<dirty>:<localDecalPositions>" (always clean after). r12: FnTiny gives odd k decals first;
    // the holder's and its children's Object.Destroys compare (their ids from the PI Clone records, dump.rs).
    ("DecalControllerBehaviour.Clear", |g, c| {
        let (pid, k) = c.field("em").ok_or("no em=")?.split_once(',').ok_or("bad em=")?;
        let pid: i32 = pid.parse().map_err(|_| "em physical")?;
        let k: usize = k.parse().map_err(|_| "em index")?;
        let p = g.physicals.iter().position(|x| x.id == pid).ok_or("no physical of the controller")?;
        if k >= g.physicals[p].decal_controllers.len() {
            return Err(format!("no decal controller {k}"));
        }
        g.decal_controller_clear(p, k);
        Ok("dc=0:".into())
    }),
    // fc41 r11: GlowInTheDarkBehaviour.Start on the flask FnTiny gave one (a runtime AddComponent ours does not load): the glow
    // joins ours by the PI Clone that lists it, owned by that Clone's FlaskBehaviour.
    ("GlowInTheDarkBehaviour.Start", |g, c| {
        let key = format!("GlowInTheDarkBehaviour:{}", c.id);
        let comps = c.s.of("Clone").filter_map(|r| r.get("comps")).find(|v| v.split([';', '[', ']']).any(|e| e == key)).ok_or("no Clone lists the glow")?;
        let fid: i32 = comps.split([';', '[', ']']).find_map(|e| e.strip_prefix("FlaskBehaviour:")).ok_or("no FlaskBehaviour beside the glow")?.parse().map_err(|_| "flask id")?;
        let f = g.flasks.iter().position(|x| x.id == fid).ok_or_else(|| format!("no flasks entry #{fid} (ours: {:?}; clone {})", g.flasks.iter().map(|x| x.id).collect::<Vec<_>>(), comps.get(..60).unwrap_or(comps)))?;
        if !g.glows.iter().any(|x| x.id == c.id) {
            g.glows.push(super::super::glow::GlowInTheDark { id: c.id, owner: Owner::Flask(f), light: None, glow_go: 0, light_go: 0 });
        }
        let i = g.glows.iter().position(|x| x.id == c.id).ok_or("no glows entry")?;
        g.glow_start(i);
        VOID
    }),
    // fc41 r11: FirearmAttachmentPointBehaviour.DisconnectAttachment: FnTiny cleared ignoreConnect first (" dap=0"); st as FnTiny's
    // "da:<ignoreConnect>,<FirearmAttachment set>". Not compared: detailViewBlob.enabled (ours draws no blob) and ConnectDelay's start.
    ("FirearmAttachmentPointBehaviour.DisconnectAttachment", |g, c| {
        let pt = g.attachment_points.iter().position(|p| p.id == c.id).ok_or("no attachment_points entry")?;
        if c.s.of("FnCase").next().and_then(|r| r.get("dap")) == Some("0") { g.attachment_points[pt].ignoreConnect = false }
        g.attachment_point_disconnect(pt);
        let p = &g.attachment_points[pt];
        Ok(format!(" st=da:{},{}", p.ignoreConnect as u8, p.FirearmAttachment.is_some() as u8))
    }),
    // fc41 r11: CenterOfGravity.Update (CenterOfBounds' too): FnTiny's seeded pre state (" cgp=enabled,x,y,scale"), which of
    // the two (" cgb="), and the selection (" cgs=<ids:>|-") into ours' marker and tool; st as FnTiny's "cg:".
    ("CenterOfGravity.Update", |g, c| {
        let case = c.s.of("FnCase").next().ok_or("no FnCase")?;
        let f = |v: &str| u32::from_str_radix(v, 16).map(f32::from_bits).map_err(|e| e.to_string());
        let cgp: Vec<&str> = case.get("cgp").ok_or("no cgp=")?.split(',').collect();
        // fc41 r12: a fifth value, the (fuzzed) SizeMultiplier.
        let (en, x, y, s, m) = match cgp[..] { [en, x, y, s] => (en, x, y, s, None), [en, x, y, s, m] => (en, x, y, s, Some(m)), _ => return Err("cgp= values".into()) };
        let bounds = case.get("cgb") == Some("1");
        g.recorded_centers()?;
        let i = g.centers.iter().position(|m| m.bounds == bounds).ok_or("no centers entry")?;
        let sel: Vec<usize> = match case.get("cgs").ok_or("no cgs=")? {
            "-" => Vec::new(),
            v => v.split(':').map(|x| x.parse::<i32>().map_err(|e| e.to_string()).and_then(|id| phys_by_id(g, id))).collect::<Result<_, _>>()?,
        };
        match g.tool.as_mut() {
            Some(t) => t.rawSelection = sel,
            None => g.tool = Some(super::super::tool::Tool { rawSelection: sel, ..Default::default() }),
        }
        if let Some(m) = m { g.centers[i].SizeMultiplier = f(m)?; }
        let r = g.centers[i].renderer;
        g.renderers[r].enabled = en == "1";
        let (x, y, s) = (f(x)?, f(y)?, f(s)?);
        g.renderers[r].chain[0].t[0] = x;
        g.renderers[r].chain[0].t[1] = y;
        g.renderers[r].chain[0].s = [s, s, s, 0.0];
        g.center_update(i);
        let h = |x: f32| format!("{:08x}", x.to_bits());
        let ch = g.renderers[r].chain[0];
        Ok(format!(" st=cg:{},{},{},{}", g.renderers[r].enabled as u8, h(ch.t[0]), h(ch.t[1]), h(ch.s[0])))
    }),
    // fc41 r10: ShockwaveBehaviour.Update; FnTiny sets the private t first and writes it with startSize (" swt=t,startSize").
    ("ShockwaveBehaviour.Update", |g, c| {
        let i = g.shockwaves.iter().position(|x| x.id == c.id).ok_or("no shockwaves entry")?;
        if let Some(v) = c.s.of("FnCase").next().and_then(|r| r.get("swt")) {
            let f: Vec<f32> = v.split(',').map(|x| u32::from_str_radix(x, 16).map(f32::from_bits).map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
            let [t, start] = f[..] else { return Err(format!("swt: {} values", f.len())) };
            (g.shockwaves[i].t, g.shockwaves[i].startSize) = (t, start);
        }
        // fc41 r11: the case's (fuzzed) Duration and SizeMultiplier from its ItemScript record.
        if let Some(r) = c.s.of("ItemScript").find(|r| r.id == c.id) {
            let f = |k: &str| r.get(k).and_then(|v| u32::from_str_radix(v, 16).ok()).map(f32::from_bits).ok_or(format!("ItemScript {k}"));
            (g.shockwaves[i].Duration, g.shockwaves[i].SizeMultiplier) = (f("Duration")?, f("SizeMultiplier")?);
        }
        g.shockwave_update(i);
        VOID
    }),
    // fc41 r10: AerialFaithPlateBehaviour.Use: the generated entry read the ActivationPropagation arg as the self's index;
    // the self is the case's target (Use ignores the activation).
    ("AerialFaithPlateBehaviour.Use", |g, c| {
        let a = g.tr.aerial_faith_plate_behaviour.iter().position(|x| x.this_id == c.id).ok_or("no aerial_faith_plate_behaviour entry")?;
        g.aerial_faith_plate_behaviour_use(a, crate::game::translated::rt::Activation { Channel: 0 });
        VOID
    }),
    // fc41 r7: SyringeBehaviour.Shot as Balloon.Shot: the WaitAFrame's wait (st=bw), on recordings whose case has " syw=1".
    ("SyringeBehaviour.Shot", |g, c| {
        let i = g.syringes.iter().position(|x| x.id == c.id).ok_or("no syringes entry")?;
        let seq = g.coroutine_seq;
        g.syringe_shot(i);
        if c.s.of("FnCase").next().and_then(|r| r.get("syw")) != Some("1") { return Ok(String::new()) }
        let Some(k) = g.coroutines.iter().position(|x| x.seq > seq && x.owner == c.id) else { return Ok(" st=bw:none".into()) };
        Ok(format!(" st=bw:{:08x}", ((g.coroutines[k].at - g.time.dynamicTimeAsDouble) as f32).to_bits()))
    }),
    ("BalloonBehaviour.Shot", |g, c| {
        let i = g.balloons.iter().position(|x| x.id == c.id).ok_or("no balloons entry")?;
        let seq = g.coroutine_seq;
        g.balloon_shot(i);
        let Some(k) = g.coroutines.iter().position(|x| x.seq > seq && x.owner == c.id) else { return Ok(" st=bw:none".into()) };
        let mut st = format!(" st=bw:{:08x}", ((g.coroutines[k].at - g.time.dynamicTimeAsDouble) as f32).to_bits());
        if c.s.of("FnCase").next().and_then(|r| r.get("balpop")) == Some("1") {
            let r = g.coroutines.remove(k);
            g.resume_coroutine(r);
            st.push_str(",pop");
        }
        Ok(st)
    }),
    // gapB r2: BalloonBehaviour.Stabbed(stabbing): Pop. MakeArg's wound=0 stabber gets StabCausesWound back before the PO (ListenPost).
    ("BalloonBehaviour.Stabbed", |g, c| { let i = g.balloons.iter().position(|x| x.id == c.id).ok_or("no balloons entry")?;
        if c.rec::<i32>("Stab(", "wound")? == 0 { let s = c.rec::<i32>("Stab(", "stabber")?; let p = phys_by_id(g, s)?; g.physicals[p].StabCausesWound = true; }
        g.balloon_stabbed(i); VOID }),
    // gapB r2: Transfer(phys, cell, multiplier): the cell by its World key (FnTiny's `cell:x:y`; the AmbientGridCells record
    // lists the cells under phys, that one among them).
    ("AmbientTemperatureGridBehaviour.Transfer", |g, c| { let w = c.with(3)?; let p = w.p_usize(g, 0)?; let k = cell_key(w.arg(1)?)?; let i = g.ambient_grid.World.find(k).ok_or(format!("no grid cell {k:?}"))?; let m = w.p_f32(2)?; g.ambient_grid_transfer(p, i, m); VOID }),
    // gapC r1: WoodPinTool.GetBreakingThreshold() takes no args; the rewrite's one fn takes `wood` (true for WoodPinTool).
    // gapC r1: gen4a records it as a PI case (FnCase, static, self=0) as well as fnpure.
    ("StatManager.IncrementInteger", |_g, c| stat_increment(c, false)),
    ("StatManager.IncrementFloat", |_g, c| stat_increment(c, true)),
    ("StatManager.UnlockAchievement", |_g, c| { let c = c.with(1)?; super::super::stats::unlock_achievement(&c.p_string(0)?); VOID }),
    // gapC r1: IEnumerators; the recorder writes the first MoveNext's yield (routine_step).
    ("GorseBlood.AcidRoutine", |g, c| { let c = c.with(2)?; let (a0, a1) = (c.p_usize(g, 0)?, c.p_usize(g, 1)?); let mut r = Game::acid_routine_body(a0, a1); Ok(routine_step(g, &mut r)) }),
    // The rewrite queues the end-of-frame rest (StartCoroutine); the recorder ran only the first MoveNext, so drop it.
    ("LimbBehaviour.CrushNextFrame", |g, c| { let l = c.with(1)?.p_usize(g, 0)?; g.limb_crush_next_frame(l); g.end_of_frame.pop(); Ok("step:WaitForEndOfFrame".into()) }),
    ("WoodPinTool.GetBreakingThreshold", |_g, c| { c.with(0)?; Ok(Enc::enc(super::super::Game::pin_tool_breaking_threshold(true))) }),
    // batch4 r3: the gen twin ran FixedUpdate. Awake only caches rb = GetComponent<Rigidbody2D>(), which the link gives
    // (vibrations[v].physical): nothing runs here and no state is compared, so no credit.
    // batch4 r3: OnAfterDeserialise ignores its List<GameObject> (with_self drops it); fn_check compares damagePoints after.
    ("SkinMaterialHandler.OnAfterDeserialise", |g, c| { let a0 = c.with_self(1)?.p_usize(g, 0)?; g.skin_on_after_deserialise(a0); VOID }),
    // batch4 r3: the recorder's direct call is IgniteRoutine's first MoveNext (Light on, the clip); no StartCoroutine.
    ("SpikeGrenadeBehaviour.IgniteRoutine", |g, c| { let i = c.with(1)?.p_usize(g, 0)?; let d = g.spike_grenade_routine_first(i); Ok(format!("step:WaitForSeconds({})", hex(d))) }),
    // batch4 r3: empty bodies in 1.28.41 (attachment.rs firearm_attachment_on_hit notes): nothing runs.
    ("ScopeAttachmentBehaviour.OnFire", |g, c| { g.firearm_attachments.iter().position(|x| x.id == c.sid).ok_or("no firearm_attachments entry")?; VOID }),
    ("ScopeAttachmentBehaviour.OnHit", |g, c| { g.firearm_attachments.iter().position(|x| x.id == c.sid).ok_or("no firearm_attachments entry")?; VOID }),
    // batch4 r3: UseBubbler.Use(signal): parent.SendMessage("Use") unless OnlyDirect and not direct (tool.rs use_receiver).
    ("UseBubbler.Use", |g, c| { let (direct, channel, _) = signal_arg(c.args.first().copied().unwrap_or("signal"))?; let act = g.signal_direct(direct, channel); g.use_bubbler_use(c.sid, &act); VOID }),
    // batch4 r3: UseBubbler.UseContinuous: AssertState (a read), then parent.SendMessage("UseContinuous"), which no
    // component of its one holder's parent (the Truck) takes (continuous.rs): nothing changes.
    ("UseBubbler.UseContinuous", |g, c| { g.with_component_instance(c.sid, |_, _, _| ()); VOID }),
    // batch4 r3: GetHitPoint(coll): SharpCollider.ClosestPoint(coll.ClosestPoint(transform.position)).
    // The PI record's softConnections (victim, collider, shouldBeDeleted). The recorder writes no FrictionJoint2D, so
    // the joints are not loaded: SoftConnect only tests the keys (Add's ArgumentException).
    ("SharpOnAllSidesBehaviour.SoftConnect", |g, c| {
        let s = g.sharp_on_all_sides.iter().position(|x| x.id == c.sid).ok_or("no SharpOnAllSidesBehaviour")?;
        let r = c.s.of("ItemScript").find(|r| r.id == c.sid).ok_or("no SharpOnAllSidesBehaviour record")?;
        let list = r.req("softConnections")?.trim_start_matches('{').trim_end_matches('}');
        let mut conns = crate::game::soft_connect::SoftConnections::default();
        for e in super::split_top(list, ';').into_iter().filter(|e| !e.is_empty()) {
            let get = |k: &str| e.split(' ').find_map(|w| w.strip_prefix(k)).map(|v| v.strip_suffix(')').unwrap_or(v)).ok_or_else(|| format!("softConnections entry without {k}: {e}"));
            let phys = phys_by_id(g, reference(get("phys=")?).ok_or("null phys")?)?;
            let coll = reference(get("coll=")?).unwrap_or(0);
            // A checker seed, not the rewrite: joint usize::MAX (the recorder writes no FrictionJoint2D; see above).
            let mut sc = crate::game::soft_connect::SoftConnection::new(usize::MAX, phys, coll);
            sc.should_be_deleted = get("shouldBeDeleted=")? == "1";
            conns.add(phys, sc);
        }
        g.sharp_on_all_sides[s].softConnections = conns;
        let other = match c.args.first().copied() { Some(a) => reference(a).map(|id| phys_by_id(g, id)).transpose()?, None => None };
        let coll = c.args.get(1).and_then(|a| reference(a));
        g.sharp_on_all_sides_soft_connect(s, other, coll, c.f(2));
        VOID
    }),
    ("SharpOnAllSidesBehaviour.GetHitPoint", |g, c| { let s = g.sharp_on_all_sides.iter().position(|x| x.id == c.sid).ok_or("no SharpOnAllSidesBehaviour")?; let coll = reference(c.args.first().ok_or("no collider")?).ok_or("null collider")?; let (p, sharp) = (g.sharp_on_all_sides[s].physical, g.sharp_on_all_sides[s].sharp_collider_id); Ok(Enc::enc(g.sharp_on_all_sides_get_hit_point(p, sharp, coll))) }),
    // batch4 r4: Update: the held-use heat counter, time += or -= deltaTime / Duration, ForceSetProgress (visual.rs visual_update).
    ("TrivialAnimationActivationBehaviour.Update", |g, c| { let v = c.with(1)?.p_visual(g, 0)?; g.visual_update(v); VOID }),
    ("VertexColourBasedOnTemperatureBehaviour.Update", |g, c| { let v = c.with(1)?.p_visual(g, 0)?; g.visual_update(v); VOID }),
    // batch4 r4: the recorder's direct call is Routine's first MoveNext (IsPlaying, AnimationTick(0) twice, WaitForEndOfFrame).
    ("TrivialAnimationBehaviour.Routine", |g, c| { let v = c.with(1)?.p_visual(g, 0)?; let mut r: crate::game::coroutine::Routine = Box::new(move |g: &mut Game| g.trivial_animation_routine_first(v)); Ok(routine_step(g, &mut r)) }),
    // batch4 r4: Blink's first MoveNext: light.color = ArmedLightColour, light toggled, WaitForSeconds(0.474).
    ("StickyGrenadeBehaviour.Blink", |g, c| { c.with(1)?; let s = g.sticky_grenades.iter().position(|x| x.id == c.sid).ok_or("no StickyGrenadeBehaviour")?; let mut k = 0u32; let mut r: crate::game::coroutine::Routine = Box::new(move |g: &mut Game| g.sticky_blink_step(s, &mut k)); Ok(routine_step(g, &mut r)) }),
    // batch4 r4: UpdateParticleSystem(progress): one Random.Range(5f, 12f); the rest is the ParticleSystem (presentation).
    ("ThrusterbedBehaviour.UpdateParticleSystem", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_f32(1)?; g.thrusterbed_update_particle_system(a0, a1); VOID }),
    ("VibrationBehaviour.Awake", |g, c| { let c = c.with(1)?; c.p_usize(g, 0)?; VOID }),
    // batch4 r3: the gen twin returned eyedrop_asset(). Main is the ToolController's SelectionController (the rewrite's Tool):
    // its fields as the recorder formats a plain object. SelectionChanged has no subscriber in 1.28 (always null).
    ("SelectionController.get_Main", |g, c| { c.with(0)?; let Some(t) = g.tool.as_ref() else { return Ok("null".into()) };
        Ok(format!("SelectionController(rawSelection={} hover={} SelectionChanged=null)", t.rawSelection.clone().enc(), t.hover.enc())) }),
    // batch2 r6: LifePoison (obsolete; FnTiny adds it on two limbs as AcidPoison). GeneralHealing heals the poison's own Limb (the
    // gen entry took the self as the limb); the Heal* iterators return their first MoveNext as the recorder writes it.
    ("LifePoison.GeneralHealing", |g, c| { c.with(1)?; let i = g.poisons.iter().position(|x| x.id == c.sid).ok_or("no poisons entry")?; let l = g.poisons[i].Limb; g.poison_general_healing(l); VOID }),
    ("LifePoison.HealBurn", |g, c| { let l = c.with(2)?.p_usize(g, 1)?; let mut r = g.heal_burn_routine(l); Ok(routine_step(g, &mut r)) }),
    ("LifePoison.HealAcidFlesh", |g, c| { let l = c.with(2)?.p_usize(g, 1)?; let mut r = Game::heal_acid_flesh_routine(l); Ok(routine_step(g, &mut r)) }),
    ("LifePoison.HealRottenFlesh", |g, c| { let l = c.with(2)?.p_usize(g, 1)?; let mut r = Game::heal_rotten_flesh_routine(l); Ok(routine_step(g, &mut r)) }),
    ("LifePoison.HealWounds", |g, c| { let l = c.with(2)?.p_usize(g, 1)?; let mut r = Game::heal_wounds_routine(l); Ok(routine_step(g, &mut r)) }),
    // batch2: the gen twin was UpdateDisplay; OnWillRenderObject is the light + gizmo scale (laser.rs key_trigger_will_render).
    ("KeyTriggerBehaviour.OnWillRenderObject", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; g.key_trigger_will_render(a0); VOID }),
    // batch3 r4: SetValue on the counter's tube (the counter that holds it, counter.rs nixie_set_value).
    ("NixieTubeBehaviour.SetValue", |g, c| { let w = c.with(2)?; let n: i32 = w.p_int(1)?; let k = g.counters.iter().position(|x| x.nixie == c.sid).ok_or("no counter with this NixieTubeBehaviour")?; g.nixie_set_value(k, n); VOID }),
    // batch3 r4: a PyroPowerTool the recorder added (AddComponent: Awake made its entity, which the record does not carry) is
    // current only after OnToolChosen, i.e. while it is the ToolController's CurrentTool; otherwise pyroEntity.IsCurrentTool is false.
    // fc41 r5: OnToolUnchosen on the self's entity (the record's pyroEntity; ours loads pyro_tool only for the current tool).
    ("PyroPowerTool.OnToolUnchosen", |g, c| { pyro_from_record(g, c)?; g.pyro_tool_unchoose(); VOID }),
    ("PyroPowerTool.Update", |g, c| { c.with(1)?; pyro_from_record(g, c)?; if g.pyro_tool.is_none() { let cur = c.s.of("ToolControllerBehaviour").next().and_then(|r| r.get("CurrentTool")).and_then(reference); if cur == Some(c.sid) { return Err("the current PyroPowerTool's entity is not in the record".into()); } return VOID; } g.pyro_tool_update(); VOID }),
    // batch3 r4: the case's own RectangleAudioSourceBehaviour (the gen entry ran every one).
    ("RectangleAudioSourceBehaviour.LateUpdate", |g, c| { c.with(1)?; g.rectangle_audio_late_update_of(Some(c.sid)); VOID }),
    // batch3 r1: PistonBehaviour.Start again on a started piston (Slider is already the adopted SliderJoint2D).
    ("PistonBehaviour.Start", |g, c| { c.with(1)?; let i = g.pistons.iter().position(|x| x.id == c.sid).ok_or("no pistons entry")?; let f = g.pistons[i].Slider.ok_or("no Slider")?; g.piston_start(i, f); VOID }),
    // ResetTransformBehaviour.ResetTransform / Start: ToReset (and ResetOnStart) from the component's ItemScript record.
    ("ResetTransformBehaviour.ResetTransform", |g, c| { c.with(1)?; let t = reset_transform_fields(c)?.1; g.reset_transform(c.sid, t); VOID }),
    // PressureGaugeBehaviour is linked as a PowerGauge (link.rs): GetProgress reads its PhysicalBehaviour field.
    ("PressureGaugeBehaviour.GetProgress", |g, c| { c.with(1)?; let i = power_gauge_of(g, c)?; let p = g.power_gauges[i].PhysicalBehaviour.ok_or("null PhysicalBehaviour")?; Ok(Enc::enc(g.power_gauge_get_progress(i, p))) }),
    ("PressureGaugeBehaviour.GetDisplayValueFor", |g, c| { let w = c.with(2)?; let i = power_gauge_of(g, c)?; Ok(Enc::enc(g.power_gauge_display_value_for(i, w.p_f32(1)?))) }),
    // ObjectGradientAnimation.Update: visual_update's SpriteGradient arm (t += deltaTime * Speed; ApplyColour).
    ("ObjectGradientAnimation.Update", |g, c| { let v = c.with(1)?.p_visual(g, 0)?; gradient_tpos(g, c, v)?; g.visual_update(v); VOID }),
    // RandomCarTextureBehaviour.Awake: no game args; `base.enabled` from the component's ItemScript record (the generated entry took it as an arg).
    ("RandomCarTextureBehaviour.Awake", |g, c| { let t = c.with(1)?.p_usize(g, 0)?; let on = c.s.of("ItemScript").find(|r| r.id == c.sid).ok_or("no RandomCarTextureBehaviour record")?.get("enabled") == Some("1"); g.random_car_texture_awake(t, on); VOID }),
    // PlayAtRandom.Start: Chance from the component's ItemScript record.
    ("PlayAtRandom.Start", |g, c| { c.with(1)?; let ch = f(c.s.of("ItemScript").find(|r| r.id == c.sid).and_then(|r| r.get("Chance")).ok_or("no PlayAtRandom Chance")?); g.play_at_random_start(c.sid, ch); VOID }),
    // PersonBehaviour.CreatePose / InitialiseAllPoses (editor helpers the probe calls): the generated entries ran person_on_flip.
    ("PersonBehaviour.CreatePose", |g, c| { let p = c.with(1)?.p_usize(g, 0)?; g.person_create_pose(p); VOID }),
    ("PersonBehaviour.InitialiseAllPoses", |g, c| { let p = c.with(1)?.p_usize(g, 0)?; g.person_initialise_all_poses(p); VOID }),
    ("ResetTransformBehaviour.Start", |g, c| { c.with(1)?; let (on, t) = reset_transform_fields(c)?; if on { g.reset_transform(c.sid, t); } VOID }),
    // batch2 r2: GearManagerBehaviour.Gears, none of it in a snapshot: BigProbeB.GearState's em= before the call, the same
    // after it as the void method's ret.
    ("GearBehaviour.Awake", |g, c| { gears_load(g, c)?; g.gear_awake(c.sid); Ok(gears_state(g)) }),
    ("GearBehaviour.OnDestroy", |g, c| { gears_load(g, c)?; g.gear_destroy(c.sid); Ok(gears_state(g)) }),
    // batch2 r7: the manager's Gears list (em=, BigProbeB.GearState) before and after the RandomComparer sort.
    ("GearManagerBehaviour.Update", |g, c| { c.with(0)?; gears_load(g, c)?; g.gear_manager_update(); Ok(gears_state(g)) }),
    // batch2 r2: Global.ClearMap(mask, resetEnvSettings) on Global.main (fnb2c_clearmap); resetEnvSettings is read only by
    // the NextFrameCoroutine, after this frame.
    ("Global.ClearMap", |g, c| {
        let w = c.with(2)?;
        let mask = w.p_int::<i32>(0)? as u32;
        let _ = w.p_bool(1)?;
        let game = crate::game_assets::find_game(None)?;
        if g.assets.is_none() { g.assets = Some(Box::new(crate::assets::db::AssetDb::open(&game)?)); }
        g.clear_map(mask);
        VOID
    }),
    // batch2 r2: a subclass's OnConnect/OnDisconnect called directly (FnTiny: odd cases connected through a pistol's point).
    ("FlashlightAttachmentBehaviour.OnConnect", |g, c| { let a = attachment_of(g, c)?; g.attachment_on_connect(a); VOID }),
    ("FlashlightAttachmentBehaviour.OnDisconnect", |g, c| { let a = attachment_of(g, c)?; g.attachment_on_disconnect(a); VOID }),
    ("ExplosiveAttachmentBehaviour.OnConnect", |g, c| explosive_em(g, c, Game::attachment_on_connect)),
    ("ExplosiveAttachmentBehaviour.OnDisconnect", |g, c| explosive_em(g, c, Game::attachment_on_disconnect)),
    // batch2 r13: Start's self is the `layer_serialisations` key (the PhysicalBehaviour beside it), not an ItemScript index.
    ("LayerSerialisationBehaviour.Start", |g, c| { let w = c.with(1)?; let id = w.sid(); let p = g.layer_serialisations.iter().find(|(_, l)| l.id == id).map(|(&p, _)| p).ok_or_else(|| format!("no layer_serialisations entry #{id}"))?; g.layer_serialisation_start(p); VOID }),
    // batch2 r3: GetTargetPos returns a Vector2 (the generated entry encoded the Vector3 it is made from).
    // batch2 r4: the controller's state from em= (BigProbeB.LeState; FnTiny seeds Targets, Container and the selection), the same after
    // the call appended to the ret. No em=: the fnb2a cases.
    ("LayerEditorController.GetTargetPos", |g, c| { c.with(0)?; let em = le_load(g, c)?; let p = g.layer_editor_get_target_pos(); Ok(le_after(g, em, Enc::enc([p[0], p[1]]))) }),
    // gapC r7: UpdateDisplays with em= too (FnTiny seeds Targets on k % 4 != 0 since gapC r7).
    ("LayerEditorController.UpdateDisplays", |g, c| { c.with(0)?; let em = le_load(g, c)?; g.layer_editor_update_displays(); Ok(le_after(g, em, String::new())) }),
    ("LayerEditorController.Show", |g, c| { c.with(0)?; let em = le_load(g, c)?; g.layer_editor_show(); Ok(le_after(g, em, String::new())) }),
    // gapB r1: Global.Start on the scene's Global (the generated entry is the stats part only, a pure twin).
    // gapB r5: with BigProbeB's em= (GlState; FnTiny sets it off on odd k) seeded first, the same state after as the void ret.
    // No em=: the fnb2r cases.
    ("Global.Start", |g, c| {
        c.with_self(1)?;
        let Some(em) = c.field("em") else { g.global_start(c.sid); return VOID };
        global_start_em(g, c.sid, em)
    }),
    // gapB r1: LimbStatusViewBehaviour (Main) from em= (BigProbeB.LvState; FnTiny seeds Limbs, t and activeSelf), its state after as the void ret.
    ("LimbStatusViewBehaviour.Refresh", |g, c| { c.with(0)?; lv_load(g, c)?; g.limb_view_refresh(); Ok(lv_after(g)) }),
    ("LimbStatusViewBehaviour.Update", |g, c| { c.with(0)?; lv_load(g, c)?; g.limb_view_update(); Ok(lv_after(g)) }),
    // batch2 r3: the scene's GlobalShrapnelEmitter with its BallisticsEmitter's state from em= (BigProbeB.Em; FnTiny seeds a fresh
    // one by Awake on odd cases), the same after the call as the void method's ret. No em=: the generated cases of fnb2a.
    ("GlobalShrapnelEmitter.OnDestroy", |g, c| {
        c.with(0)?;
        let em = c.field("em");
        if let Some(v) = em {
            super::fnbigb::em_load(&mut g.shrapnel.as_mut().ok_or("no GlobalShrapnelEmitter")?.emitter, v)?;
        }
        g.global_shrapnel_on_destroy();
        Ok(match em { Some(_) => format!("em={}", super::fnbigb::em_text(&g.shrapnel.as_ref().ok_or("no GlobalShrapnelEmitter")?.emitter)), None => String::new() })
    }),
    // batch2 r10: EmitShrapnel the same way (em= in and out); its routine is compared after (ShrapnelRoutines).
    ("GlobalShrapnelEmitter.EmitShrapnel", |g, c| {
        let w = c.with(2)?;
        let em = c.field("em");
        if let Some(v) = em {
            super::fnbigb::em_load(&mut g.shrapnel.as_mut().ok_or("no GlobalShrapnelEmitter")?.emitter, v)?;
        }
        g.global_shrapnel_emit_shrapnel(w.p_arr::<2>(0)?, w.p_int(1)?);
        Ok(match em { Some(_) => format!("em={}", super::fnbigb::em_text(&g.shrapnel.as_ref().ok_or("no GlobalShrapnelEmitter")?.emitter)), None => String::new() })
    }),
    // batch3 r1: PistonBehaviour.Start again on a started piston (Slider is already the adopted SliderJoint2D).
    ("PistonBehaviour.Start", |g, c| { c.with(1)?; let i = g.pistons.iter().position(|x| x.id == c.sid).ok_or("no pistons entry")?; let f = g.pistons[i].Slider.ok_or("no Slider")?; g.piston_start(i, f); VOID }),
    // ResetTransformBehaviour.ResetTransform / Start: ToReset (and ResetOnStart) from the component's ItemScript record.
    ("ResetTransformBehaviour.ResetTransform", |g, c| { c.with(1)?; let t = reset_transform_fields(c)?.1; g.reset_transform(c.sid, t); VOID }),
    // PressureGaugeBehaviour is linked as a PowerGauge (link.rs): GetProgress reads its PhysicalBehaviour field.
    ("PressureGaugeBehaviour.GetProgress", |g, c| { c.with(1)?; let i = power_gauge_of(g, c)?; let p = g.power_gauges[i].PhysicalBehaviour.ok_or("null PhysicalBehaviour")?; Ok(Enc::enc(g.power_gauge_get_progress(i, p))) }),
    ("PressureGaugeBehaviour.GetDisplayValueFor", |g, c| { let w = c.with(2)?; let i = power_gauge_of(g, c)?; Ok(Enc::enc(g.power_gauge_display_value_for(i, w.p_f32(1)?))) }),
    // ObjectGradientAnimation.Update: visual_update's SpriteGradient arm (t += deltaTime * Speed; ApplyColour).
    ("ObjectGradientAnimation.Update", |g, c| { let v = c.with(1)?.p_visual(g, 0)?; gradient_tpos(g, c, v)?; g.visual_update(v); VOID }),
    // RandomCarTextureBehaviour.Awake: no game args; `base.enabled` from the component's ItemScript record (the generated entry took it as an arg).
    ("RandomCarTextureBehaviour.Awake", |g, c| { let t = c.with(1)?.p_usize(g, 0)?; let on = c.s.of("ItemScript").find(|r| r.id == c.sid).ok_or("no RandomCarTextureBehaviour record")?.get("enabled") == Some("1"); g.random_car_texture_awake(t, on); VOID }),
    // PlayAtRandom.Start: Chance from the component's ItemScript record.
    ("PlayAtRandom.Start", |g, c| { c.with(1)?; let ch = f(c.s.of("ItemScript").find(|r| r.id == c.sid).and_then(|r| r.get("Chance")).ok_or("no PlayAtRandom Chance")?); g.play_at_random_start(c.sid, ch); VOID }),
    // PersonBehaviour.CreatePose / InitialiseAllPoses (editor helpers the probe calls): the generated entries ran person_on_flip.
    ("PersonBehaviour.CreatePose", |g, c| { let p = c.with(1)?.p_usize(g, 0)?; g.person_create_pose(p); VOID }),
    ("PersonBehaviour.InitialiseAllPoses", |g, c| { let p = c.with(1)?.p_usize(g, 0)?; g.person_initialise_all_poses(p); VOID }),
    ("ResetTransformBehaviour.Start", |g, c| { c.with(1)?; let (on, t) = reset_transform_fields(c)?; if on { g.reset_transform(c.sid, t); } VOID }),
    // fnbatch r1 (fnb1_gen): coroutines' first MoveNext (the generated entries returned void) and arity pairings.
    // BeamRepeaterBehaviour.FireCoroutine runs to its WaitForSeconds(0.02f) at once.
    // AcidPoison.Acid: its first MoveNext (one acid_step), WaitForSeconds(0.03f) or the end.
    ("AcidPoison.Acid", |g, c| { let i = c.with(1)?.p_usize(g, 0)?; let l = g.poisons[i].Limb; Ok(match g.acid_step(l) { super::super::coroutine::Yield::Done => "step:end".into(), _ => format!("step:WaitForSeconds({})", hex(0.03)) }) }),
    ("BeamRepeaterBehaviour.FireCoroutine", |g, c| { let i = c.with(1)?.p_usize(g, 0)?; g.beam_repeater_fire(i); Ok(format!("step:WaitForSeconds({})", hex(0.02))) }),
    // BeamCannonBehaviour.BlinkBeam: the line and the glow on, then WaitForSeconds(0.05f) (the generated entry passed `on`).
    ("BeamCannonBehaviour.BlinkBeam", |g, c| { let i = c.with(1)?.p_usize(g, 0)?; g.beam_cannon_blink(i, true); Ok(format!("step:WaitForSeconds({})", hex(0.05))) }),
    // batch4 r10: Transmit(name, content): FnTiny's content is new ActivationPropagation(root, 0[, gameObject]) (the record's
    // `signal:0,0,0` drops the path), so it is rebuilt from the self's root.
    ("TransmitterBehaviour.Transmit", |g, c| { let w = c.with(3)?; let t = w.p_usize(g, 0)?; let name = w.p_string(1)?; let root = g.root_id(g.transmitters[t].phys); let content = g.signal_from(root, 0); g.transmitter_transmit(t, &name, &content); VOID }),
    // UseContinuous(ActivationPropagation): GenRun's `signal:<direct>,<channel>,<target>` (the gen entry read it as the self).
    ("TransmitterBehaviour.UseContinuous", |g, c| { let t = c.record_index(g, c.sid)?.1; let (direct, channel, _) = signal_arg(c.args.first().copied().unwrap_or("signal"))?; let act = g.signal_direct(direct, channel); let n0 = g.signals.currently_active(); g.transmitter_use_continuous(t, &act); signal_sets_state(g, c, n0) }),
    ("TransmitterBehaviour.Blink", |g, c| { let t = c.with(1)?.p_usize(g, 0)?; let mut r = g.transmitter_blink_routine(t); Ok(routine_step(g, &mut r)) }),
    // FixedUpdate() of the self: the generated entry passed the component id as a game arg.
    ("TruckDoorBehaviour.FixedUpdate", |g, c| { let t = c.with(1)?.p_usize(g, 0)?; let id = g.truck_doors[t].0; g.truck_door_fixed_update(t, id); VOID }),
    // Static GetLayerAbove/GetLayerUnder(int layervalue, out SortingLayer layer): the bool, out= the value and the layer
    // (the end layer itself at the top/bottom, default(SortingLayer) for a value of no layer).
    // OnPoolableInitialised(pool): the arg's ObjectPoolBehaviour as the rewrite's pool (the generated entry took the index
    // among PoolGeneratorPool records).
    ("ReturnToPoolAfterTime.OnPoolableInitialised", |g, c| {
        let pool = g.pool_ref_of(c.s, c.args[0])?;
        g.return_to_pool_on_poolable_initialised(c.sid, pool);
        VOID
    }),
    // GenerateBasePose: the recorder writes a RagdollPose return as its Name ("Pose N", spaces as _).
    // fnbatch r3: then `/rigidity=<hex>/angles=[<limb id>:<Animated>:<Angle>:<StartAngle>:<EndAngle>,..]`.
    ("PersonBehaviour.GenerateBasePose", |g, c| {
        let c = c.with(1)?;
        let a0 = c.p_usize(g, 0)?;
        let p = g.generate_base_pose(a0);
        let angles = p.Angles.iter().map(|a| format!("{}:{}:{}:{}:{}", g.limbs[a.Limb].id, bit(a.Animated), hex(a.Angle), hex(a.StartAngle), hex(a.EndAngle))).collect::<Vec<_>>().join(",");
        Ok(format!("RagdollPose:\"{}\"/rigidity={}/angles=[{angles}]", p.Name.replace(' ', "_"), hex(p.Rigidity)))
    }),
    // ActOnJointBreak2D(joint): the arg's HingeJoint2D as the hinge of the limb whose LimbBehaviour record names it as Joint
    // (None for any other joint).
    ("CirculationBehaviour.ActOnJointBreak2D", |g, c| {
        // The self's limb by its record's Limb= (the arena is in LimbBehaviour record order); the arg's hinge is the Joint of
        // the limb whose LimbBehaviour record names it (None for any other joint).
        let me = c.s.of("CirculationBehaviour").find(|r| r.id == c.sid).ok_or("no CirculationBehaviour record")?;
        let limb_id = me.get("Limb").and_then(reference).ok_or("no Limb")?;
        let l = g.limbs.iter().position(|x| x.id == limb_id).ok_or("no limb")?;
        let id = reference(c.args[0]).ok_or("joint arg is no reference")?;
        let owner = c.s.of("LimbBehaviour").find(|r| r.get("Joint").and_then(reference) == Some(id)).map(|r| r.id);
        let joint = owner.and_then(|o| g.limbs.iter().find(|x| x.id == o)).and_then(|x| x.Joint);
        g.circulation_on_joint_break(l, joint);
        VOID
    }),
    // fnbatch r4 audit: instance methods whose GEN entry took the game's arg 0 as the self (with(n) for n recorded args:
    // off 0, so p_usize(g, 0) read the argument). The self here is param 0 (with(n + 1)) or the case's self record.
    // set_IsFlipped(value): the auto-property setter (GEN paired it to OnFlip, reading the bool arg as the person index).
    ("HingeJointLimitAutofixBehaviour.set_IsFlipped", |g, c| { let w = c.with(2)?; let p = w.p_usize(g, 0)?; g.people[p].IsFlipped = w.p_bool(1)?; VOID }),
    // ShatterProcedurally(shatterFlags): any ShatterFlags byte.
    ("LimbBehaviour.ShatterProcedurally", |g, c| { let w = c.with(2)?; let l = w.p_usize(g, 0)?; g.limb_shatter_procedurally(l, w.p_int(1)?); VOID }),
    ("PersonBehaviour.Shot", |g, c| { let p = c.with_shot(1)?.p_usize(g, 0)?; g.person_shot(p); VOID }),
    // OnJointBreak2D(joint): only the string's own Joint (its record's) snaps it; the self's limb by the record's name.
    ("GoreStringBehaviour.OnJointBreak2D", |g, c| {
        let me = c.s.of("GoreStringBehaviour").find(|r| r.id == c.sid).ok_or("no GoreStringBehaviour record")?;
        let l = c.s.of("LimbBehaviour").position(|r| r.name == me.name).ok_or("gore string without limb")?;
        if reference(c.args.first().ok_or("no joint arg")?).is_some() && me.get("Joint").and_then(reference) == reference(c.args[0]) {
            g.gore_string_on_joint_break(l);
        }
        VOID
    }),
    // OnAfterDeserialise(gameObjects): the list is unused; the self is param 0.
    // gapB r23: OnJointBreak2D(Joint2D joint): the joint's break-limit key (none: not a joint any helper keeps, nothing set).
    ("JointSerialisationHelper.OnJointBreak2D", |g, c| {
        let c = c.with(2)?;
        let h = c.p_usize(g, 0)?;
        let id = reference(c.arg(1)?).ok_or("null joint")?;
        if let Some(j) = g.break_joint_of_component(id) { g.joint_helper_on_joint_break(h, j) }
        VOID
    }),
    ("PoolableParticleSystemHelper.OnParticleSystemStopped", |g, c| { let c = c.with(0)?; g.poolable_particle_stopped(c.sid()); VOID }),
    ("JointBreakSoundBehaviour.OnJointBreak2D", |g, c| {
        let c = c.with(2)?;
        let s = g.joint_break_sounds.iter().position(|x| x.id == c.sid()).ok_or("no joint_break_sounds entry")?;
        let id = reference(c.arg(1)?).ok_or("null joint")?;
        if let Some(j) = g.break_joint_of_component(id) { g.joint_break_sound_on_joint_break(s, j) }
        VOID
    }),
    ("JointSerialisationHelper.OnAfterDeserialise", |g, c| { let h = c.with(2)?.p_usize(g, 0)?; g.joint_helper_on_after_deserialise(h); VOID }),
    ("BatchJointSerialisationHelper.OnAfterDeserialise", |g, c| { let h = c.with(2)?.p_usize(g, 0)?; g.joint_helper_on_after_deserialise(h); VOID }),
    // DelayedSplash(d): the coroutine's first MoveNext is `yield return new WaitForSeconds(d)` (the splash is the next step).
    // gapB r27: UnderwaterExplosionBehaviour.Start: the DelayedSplash it starts is compared (FnResult " uwd="). checker limit: the PI's
    // pending DelayedSplash routines are not recorded; ours' are dropped before the call, so only this call's is left.
    ("UnderwaterExplosionBehaviour.Start", |g, c| {
        c.with(0)?;
        let i = g.underwater_explosions.iter().position(|u| u.id == c.sid).ok_or("no underwater_explosions entry")?;
        g.coroutines.retain(|x| !matches!(x.step, crate::game::destroyable::Step::UnderwaterSplash(j) if j == i));
        g.underwater_explosion_start_at(i);
        VOID
    }),
    ("UnderwaterExplosionBehaviour.DelayedSplash", |g, c| { let w = c.with(2)?; w.p_usize(g, 0)?; Ok(format!("step:WaitForSeconds({})", hex(w.p_f32(1)?))) }),
    // RemoveAudioSource(audioSource): Global.audioSources is not modelled (GEN paired it to AntennaReceiverBehaviour.OnDestroy).
    // fnbatch r4: CirculationBehaviour.Stabbed(stab): the self's circulation (record order = limb order) and the Stabbing's fields.
    ("CirculationBehaviour.Stabbed", |g, c| {
        // The recorder's `Stab(stabber=<id> wound= normal= point=)` (MapConfigProbe); the victim is the limb's physical. The
        // stabber's StabCausesWound is the snapshot's (the arg's wound= was formatted before the case's fuzz set it).
        let l = c.comp("CirculationBehaviour")?;
        let stabber = phys_by_id(g, c.rec::<i32>("Stab(", "stabber")?)?;
        let s = super::super::stab::Stabbing { stabber, victim: g.limbs[l].PhysicalBehaviour, normal: c.rec("Stab(", "normal")?, point: c.rec("Stab(", "point")? };
        g.circulation_stabbed(l, s);
        // MapConfigProbe.MakeArg turns the stabber's StabCausesWound off for every other case (wound=0) and ListenPost
        // turns it back on after the call, before the PO snapshot: the probe's own write, done here as well.
        if c.rec::<i32>("Stab(", "wound")? == 0 {
            g.physicals[stabber].StabCausesWound = true;
        }
        VOID
    }),
    // Start(): the whole override chain (SpringCableBehaviour.Start, then the bandage's). A direct call runs it again on a
    // started wire: blood_wire_start's `started` test is DelayedStartCall's, not Start's.
    ("AppliedBandageBehaviour.Start", |g, c| { let w = c.with(1)?.p_usize(g, 0)?; g.blood_wires[w].started = false; g.blood_wire_start(w); VOID }),
    // Use(activation): the counter by its id; the activation's channel.
    ("CounterBehaviour.Use", |g, c| {
        let k = g.counters.iter().position(|x| x.id == c.sid).ok_or("no counters entry")?;
        let (_, channel, _) = signal_arg(c.args.first().copied().unwrap_or("signal"))?;
        g.counter_use(k, channel);
        VOID
    }),
    // OnFlip(center): the self (GEN read the Vector3 arg as the id).
    ("MaintainScaleSignBehaviour.OnFlip", |g, c| { let id = c.with_self(1)?.p_int(0)?; g.maintain_scale_start(id); VOID }),
    ("Global.RemoveAudioSource", |_, _| Err("no rewrite function: Global.audioSources is not modelled".into())),
    ("SortingLayers.GetLayerAbove", |g, c| { let _ = g; sorting_neighbour(c, true) }),
    ("SortingLayers.GetLayerUnder", |g, c| { let _ = g; sorting_neighbour(c, false) }),
    // fntiny r2: hand pairings for the generated entries that took the wrong arity (fnt1_gen).
    // SetTint(color) on the self's editable_tints slot.
    ("EditableTintBehaviour.SetTint", |g, c| { let i = match g.script_slots.get(&c.sid) { Some(&super::super::save_slots::Slot::EditableTints(i)) => i, _ => return Err(format!("no EditableTint slot #{}", c.sid)) }; let w = c.with(1)?; let color = w.p_arr::<4>(0)?; g.editable_tint_set_tint(i, color); VOID }),
    ("ScopeAttachmentBehaviour.OnConnect", |g, c| { let a = g.firearm_attachments.iter().position(|x| x.id == c.sid).ok_or("no firearm_attachments entry")?; g.attachment_on_connect(a); VOID }),
    ("ScopeAttachmentBehaviour.OnDisconnect", |g, c| { let a = g.firearm_attachments.iter().position(|x| x.id == c.sid).ok_or("no firearm_attachments entry")?; g.attachment_on_disconnect(a); VOID }),
    ("FirearmAttachmentBehaviour.Connect", |g, c| { let a = g.firearm_attachments.iter().position(|x| x.id == c.sid).ok_or("no firearm_attachments entry")?; g.firearm_attachment_connect(a); VOID }),
    // Use(propagation): the signal's Target, the attachment's own GameObject (its physical) or another (never equal).
    ("FirearmAttachmentBehaviour.Use", |g, c| { let a = g.firearm_attachments.iter().position(|x| x.id == c.sid).ok_or("no firearm_attachments entry")?; let (_, _, t) = signal_arg(c.args.first().copied().unwrap_or("signal"))?; let own = t != 0 && c.s.of("Clone").any(|r| r.id == t && r.get("comps").is_some_and(|v| super::list(v).iter().any(|x| x.rsplit_once(':').is_some_and(|y| y.1 == c.sid.to_string())))); let target = if own { g.firearm_attachments[a].physical } else if t != 0 { Some(usize::MAX) } else { None }; g.firearm_attachment_use(a, target); VOID }),
    // fc41 r20: the message itself (the generated entry called its GetPhysicalBehaviour helper and returned its value).
    ("StickyGrenadeBehaviour.OnCollisionEnter2D", |g, c| { let a0 = c.with(1)?.p_collision(0)?; let s = g.sticky_grenades.iter().position(|x| x.id == c.sid).ok_or("no StickyGrenadeBehaviour")?; g.sticky_grenade_collision_enter(s, &a0); VOID }),
    // fc41 r20: ObjectPoolBehaviour.Return(poolable) on the BulletTracerPool. `!poolable || !createdObjects.Contains(poolable)`:
    // Debug.LogError (console only) and nothing else. Ours counts the tracer clones without their ids: with none created
    // the argument can't be one; with some, the case is not checked (an error, never a pass).
    ("ObjectPoolBehaviour.Return", |g, c| {
        c.with(1)?;
        if g.tracer_pool.created != 0 {
            return Err("checker limit: a tracer pool with clones (ours keeps no clone ids)".into());
        }
        g.tracer_return(false);
        VOID
    }),
    // fc41 r20: the case's calibratedHoverHeights (its ItemScript record, `[h;h;..]` float bits): Awake's, not the fuzzed HoverHeight's.
    ("HovercarBehaviour.FixedUpdate", |g, c| {
        let i = g.hovercars.iter().position(|x| x.id == c.id).ok_or("no hovercars entry")?;
        let id = c.id.to_string();
        let rec = c.s.of("ItemScript").find(|r| r.id.to_string() == id && r.get("class") == Some("HovercarBehaviour")).ok_or("no HovercarBehaviour record")?;
        let v = rec.req("calibratedHoverHeights")?;
        let v = v.strip_prefix('[').and_then(|x| x.strip_suffix(']')).ok_or("calibratedHoverHeights: not an array")?;
        g.hovercars[i].calibrated = if v.is_empty() { Vec::new() } else { v.split(';').map(|w| u32::from_str_radix(w, 16).map(f32::from_bits).map_err(|e| e.to_string())).collect::<Result<_, _>>()? };
        g.hovercar_fixed_update(i);
        VOID
    }),
    ("StickyGrenadeBehaviour.OnEMPHit", |g, c| { let s = g.sticky_grenades.iter().position(|x| x.id == c.sid).ok_or("no StickyGrenadeBehaviour")?; g.sticky_grenade_on_emp_hit(s); VOID }),
    // batch4 r6: GetPhysicalBehaviour(collision, out phys): the bool, out= the collision and the phys.
    ("StickyGrenadeBehaviour.GetPhysicalBehaviour", |g, c| { if c.args.len() != 2 { return Err(format!("arity: 2 args, the game passed {}", c.args.len())) } let col = c.with(2)?.p_collision(0)?; let p = g.sticky_get_physical_behaviour(&col); Ok(format!("{} out={};{}", bit(p.is_some()), c.args[0], p.map_or("null".to_string(), |i| i.to_string()))) }),
    // batch4 r6: UseEventTrigger.Use(activation): the Event's persistent calls (use_events by the component id).
    ("UseEventTrigger.Use", |g, c| { if !g.use_events.contains_key(&c.sid) { return Err("no use_events entry".into()) } g.use_event_trigger_use(c.sid); VOID }),
    ("TemperatureGaugeBehaviour.GetProgress", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; if !g.power_gauges[a0].temperature { return Err("not a TemperatureGaugeBehaviour".into()) } let p = g.power_gauges[a0].PhysicalBehaviour.ok_or("PhysicalBehaviour null")?; Ok(Enc::enc(g.power_gauge_get_progress(a0, p))) }),
    ("TemperatureGaugeBehaviour.GetDisplayValueFor", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; if !g.power_gauges[a0].temperature { return Err("not a TemperatureGaugeBehaviour".into()) } let a1 = c.p_f32(1)?; Ok(Enc::enc(g.power_gauge_display_value_for(a0, a1))) }),
    ("ParticleMachineBehaviour.Use", |g, c| { let Some(&(_, super::super::Behaviour::ParticleMachine(m))) = g.behaviour_of.get(&c.sid) else { return Err("no ParticleMachine".into()) }; g.particle_machine_use(m); VOID }),
    // OnEnable: UpdateActivation (the direct call has enabled true). OnDisable of the component (FnTiny disabled it first):
    // every system stops.
    ("ParticleMachineBehaviour.OnEnable", |g, c| { let Some(&(_, super::super::Behaviour::ParticleMachine(m))) = g.behaviour_of.get(&c.sid) else { return Err("no ParticleMachine".into()) }; g.particle_machine_update_activation(m, false); VOID }),
    ("ParticleMachineBehaviour.OnDisable", |g, c| { g.particle_machine_on_disable(c.sid, true, false); VOID }),
    ("ParticleMachineBehaviour.UpdateActivation", |g, c| { let Some(&(_, super::super::Behaviour::ParticleMachine(m))) = g.behaviour_of.get(&c.sid) else { return Err("no ParticleMachine".into()) }; g.particle_machine_update_activation(m, false); VOID }),
    // batch4 r7: Use as UseContinuous (the gen pairing took 3 args).
    ("SignalConverterBehaviour.Use", |g, c| { let i = c.record_index(g, c.sid)?.1; let (direct, channel, _) = signal_arg(c.args.first().copied().unwrap_or("signal"))?; g.signal_converter_use(i, direct, channel); VOID }),
    // gapB r18: ConvertChannel(incoming, out outgoing): ret, then out= the args after the call (incoming as passed; outgoing); the
    // bare "signal" (Direct, channel 0) is filtered out of the case args.
    ("SignalConverterBehaviour.ConvertChannel", |g, c| { let sig = match c.args[..] { [_] => "signal", [a, _] => a, _ => return Err(format!("arity: 2 args, the game passed {}", c.args.len())) }; let i = c.record_index(g, c.sid)?.1; let (direct, channel, _) = signal_arg(sig)?; let r = g.signal_converters[i].convert_channel(direct, channel); Ok(format!("{} out={sig};{}", bit(r.is_some()), r.unwrap_or(channel))) }),
    // gapB r18: DragTool.DragStrengthFunction (GentleDragTool's override through the same fn): Rigidbody.mass of the tool's body.
    ("DragTool.DragStrengthFunction", |g, c| { c.with(0)?; let b = g.tool.as_ref().ok_or("no drag tool")?.body; Ok(Enc::enc(g.drag_tool_drag_strength(b))) }),
    // gapB r19: ModAPI.Invoke<X>(sender, args): ModApiEvent::invoke with a probe subscriber (as Recorder.cs GenCase adds to the
    // game's On<X>) that records each (sender, args) it gets: " mrx=<sender id>,<args>;...".
    ("ModAPI.InvokeWireCreated", |_, c| mod_rx(&mod_api::OnWireCreated, c, mod_id(c)?, show_id)),
    ("ModAPI.InvokePinCreated", |_, c| mod_rx(&mod_api::OnPinCreated, c, mod_id(c)?, show_id)),
    ("ModAPI.InvokeLinkCreated", |_, c| mod_rx(&mod_api::OnLinkCreated, c, mod_id(c)?, show_id)),
    ("ModAPI.InvokeWireDestroyed", |_, c| mod_rx(&mod_api::OnWireDestroyed, c, mod_id(c)?, show_id)),
    ("ModAPI.InvokePinDestroyed", |_, c| mod_rx(&mod_api::OnPinDestroyed, c, mod_id(c)?, show_id)),
    ("ModAPI.InvokeLinkDestroyed", |_, c| mod_rx(&mod_api::OnLinkDestroyed, c, mod_id(c)?, show_id)),
    ("ModAPI.InvokeItemSelected", |_, c| mod_rx(&mod_api::OnItemSelected, c, mod_id(c)?, show_id)),
    ("ModAPI.InvokeItemDeselected", |_, c| mod_rx(&mod_api::OnItemDeselected, c, mod_id(c)?, show_id)),
    ("ModAPI.InvokeItemActivated", |_, c| mod_rx(&mod_api::OnItemActivated, c, mod_id(c)?, show_id)),
    ("ModAPI.InvokeGunShot", |_, c| mod_rx(&mod_api::OnGunShot, c, mod_id(c)?, show_id)),
    ("ModAPI.InvokeDeath", |_, c| mod_rx(&mod_api::OnDeath, c, mod_id(c)?, show_id)),
    ("ModAPI.InvokeItemSpawned", |_, c| mod_rx(&mod_api::OnItemSpawned, c, mod_spawn(c)?, show_spawn)),
    ("ModAPI.InvokeBeforeItemSpawned", |_, c| mod_rx(&mod_api::OnBeforeItemSpawned, c, mod_spawn(c)?, show_spawn)),
    ("ModAPI.InvokeItemRemoved", |_, c| mod_rx(&mod_api::OnItemRemoved, c, mod_spawn(c)?, show_spawn)),
    // gapB r19: FragmentationRay(index, rayCount, position, range, fragmentForce, dismemberChance): the rewrite takes the
    // index and an ExplosionParameters holding the rest.
    ("ExplosionCreator.FragmentationRay", |g, c| {
        let w = c.with(6)?;
        let ex = crate::game::explosion::ExplosionParameters { FragmentationRayCount: w.p_int(1)?, Position: w.p_arr::<3>(2)?, Range: w.p_f32(3)?, FragmentForce: w.p_f32(4)?, DismemberChance: w.p_f32(5)?, ..Default::default() };
        g.fragmentation_ray(w.p_int::<i32>(0)? as u32, ex);
        VOID
    }),
    // gapB r19: DetectorBehaviour.FitsCriteria(laserHit): the recorder's "laser:" words (MapConfigProbe.Laser); the hit's
    // RaycastHit2D is only tested for a value, so a hit is the physical's own body at the hit point.
    ("DetectorBehaviour.FitsCriteria", |g, c| {
        c.with(2)?;
        let i = g.detectors.iter().position(|d| d.id == c.sid).ok_or("no DetectorBehaviour")?;
        let h = laser_hit_arg(g, c.args[0])?;
        Ok(Enc::enc(g.detector_fits_criteria(i, Some(h))))
    }),
    // gapB r19: PlayClipOnce(clip[, volume]): the clip by its recorded name (the PlayOneShot log names it; shots= compare).
    ("PhysicalBehaviour.PlayClipOnce", |g, c| {
        if c.args.is_empty() || c.args.len() > 2 { return Err(format!("arity: 1 or 2 args, the game passed {}", c.args.len())) }
        let p = phys_by_id(g, c.sid)?;
        let clip = c.args[0].strip_prefix('@').and_then(|x| x.rsplit_once('#')).map(|(n, _)| std::sync::Arc::new(super::super::audio::Clip {
            asset: Default::default(),
            clip: crate::assets::classes::AudioClip { name: n.to_string(), load_type: 0, channels: 1, frequency: 44100, length: 1.0, resource: Default::default(), compression_format: 0 },
            pcm: None,
            stream: None,
        }));
        let volume = if c.args.len() == 2 { item_f32(c.args[1])? } else { 1.0 };
        g.play_clip_once(p, clip, volume);
        VOID
    }),
    ("SignalConverterBehaviour.UseContinuous", |g, c| { let i = c.record_index(g, c.sid)?.1; let (direct, channel, _) = signal_arg(c.args.first().copied().unwrap_or("signal"))?; g.signal_converter_use_continuous(i, direct, channel); VOID }),
    // Static TryGetLavaAtPoint(Vector3 point, out LavaBehaviour lava): the bool, out= the point and the lava.
    ("LavaBehaviour.TryGetLavaAtPoint", |g, c| { if c.args.len() != 2 { return Err(format!("arity: 2 args, the game passed {}", c.args.len())) } let w = c.with(2)?; let p = w.p_arr::<2>(0)?; let lv = g.lava_at_point(p); Ok(format!("{} out={};{}", bit(lv.is_some()), c.args[0], lv.map_or("null".to_string(), |i| i.to_string()))) }),
    // OnImpactCreated(gm): gm's BloodImpactBehaviour, a pooled BloodPuff (GenRun's gm is a limb, which has none).
    ("CirculationBehaviour.OnImpactCreated", |g, c| { let l = c.comp("CirculationBehaviour")?; let gm = impact_gm(g, c)?; g.circulation_on_impact_created(l, gm);
        // fnbatch r6: a BloodPuff gm's splat DecalColourMultiplier after SetColor (the recorder's " dcm=").
        Ok(gm.map_or(String::new(), |b| format!(" dcm={}", g.splats[g.blood_puffs[b].splat].DecalColourMultiplier.map(|v| format!("{:08x}", v.to_bits())).join(",")))) }),
    // SetColor(color): self's blood_puffs entry is the one whose splat is its FancyBloodSplatController link (record_index counts
    // ItemScript rows, not the pool's createdObjects order).
    ("BloodImpactBehaviour.SetColor", |g, c| { let w = c.with(1)?; let r = c.s.of("ItemScript").find(|r| r.id == c.sid).ok_or("no BloodImpactBehaviour record")?;
        let sp = reference(r.req("FancyBloodSplatController")?).ok_or("null FancyBloodSplatController")?;
        // The item load (link_recording_item) makes the emitter's splat but no blood_puffs entry: add the puff for it.
        let s = g.splats.iter().position(|s| s.id == sp).ok_or_else(|| format!("no splat #{sp}"))?;
        let b = match g.blood_puffs.iter().position(|p| p.splat == s) { Some(b) => b, None => { g.blood_puffs.push(crate::game::misc::BloodPuff { active: true, splat: s, emitter_active: true, off: Vec::new() }); g.blood_puffs.len() - 1 } };
        g.blood_impact_set_color(b, w.p_arr::<4>(0)?); VOID }),
    ("PseudoBloodImpactHelper.OnImpactCreated", |g, c| { let h = g.pseudo_blood_helpers.iter().position(|h| h.0 == c.sid).ok_or("no pseudo_blood_helpers entry")?; if c.args.first().is_some_and(|a| matches!(*a, "null" | "destroyed")) { return crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object") } let gm = impact_gm(g, c)?; g.pseudo_blood_impact_on_impact_created(h, gm);
        // fc41 r8: a seeded puff case (" pbs=1") has the puff's DecalColourMultiplier after the call (FnTiny StateOf st=dcm:).
        if c.s.of("FnCase").next().and_then(|r| r.get("pbs")) != Some("1") { return VOID }
        Ok(format!(" st=dcm:{}", gm.map_or("none".into(), |b| g.splats[g.blood_puffs[b].splat].DecalColourMultiplier.map(|v| format!("{:08x}", v.to_bits())).join(",")))) }), // fc41 r7: gm.TryGetComponent on a null/destroyed gm (fc41_13_gen)
    // Affect(Collider2D coll): the collider's id (no record of its own) and its physical.
    // batch2 r4: HealthGunBehaviour.Affect the same way (the generated entry took the collider for a limb record).
    ("HealthGunBehaviour.Affect", |g, c| { let i = g.effect_guns.iter().position(|x| x.id == c.sid).ok_or("no effect_guns entry")?; let col = reference(c.args.first().ok_or("no collider")?).ok_or("null collider")?; let p = g.physicals.iter().position(|p| p.colliders.contains(&col)); g.effect_gun_affect(i, p, col); VOID }),
    // batch2 r4: Instantiate() takes no args (the generated entry passed the id and Start's `fresh` as two).
    // batch2 r7: Hover.CheckMouseInput on a WireBehaviour (the blood_wires arena); the other Hover classes (LinkDevice, Pin,
    // SerpentineBelt, VisualDeletableDetachedTool) inline it in their Update and are not wired here.
    // batch2 r9: the self chunk's FuzzySetVolume (the gen entry was its AddComponent half, fire_loop_add_source).
    // gapC r14: the regen (a0341c9b) dropped this entry; the Option Fingerprint (null vs empty) is what it checks.
    ("PoisonSpreadBehaviour.GetFingerprintValue", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; Ok(Enc::enc(g.poison_fingerprint_value(a0))) }),
    // gapC r14: the real FixedUpdate (the GEN entry ran the translated one, which drops FuzzySetVolumeAt).
    // gapC r14: no entry listed these (the singletons' self is not a rewrite instance; the rewrite keeps one of each).
    ("FireLoopSoundControllerBehaviour.FuzzySetVolumeAt", |g, c| { let c = c.with(3)?; let (p, v, w) = (c.p_arr::<3>(0)?, c.p_f32(1)?, c.p_f32(2)?); g.fire_loop_fuzzy_set_volume_at(p, v, w); VOID }),
    ("CameraShakeBehaviour.Shake", |g, c| { let c = c.with(3)?; let (i, p, d) = (c.p_f32(0)?, c.p_arr::<2>(1)?, c.p_f32(2)?); g.camera_shake_at(i, p, d); VOID }),
    ("FireSoundEmitter.FixedUpdate", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; g.fire_sound_emitter_fixed(a0); VOID }),
    ("FireLoopChunkBehaviour.FuzzySetVolume", |g, c| { let c = c.with(2)?; let key = *g.fire_loop_chunks.iter().find(|(_, k)| k.id == c.sid()).ok_or("the self is not a FireLoopChunk")?.0; let (v, w) = (c.p_f32(0)?, c.p_f32(1)?); g.fire_loop_chunk_fuzzy_set_volume(key, v, w); VOID }),
    ("Hover.CheckMouseInput", |g, c| { c.with(0)?; let w = g.blood_wires.iter().position(|x| x.id == c.sid).ok_or("Hover.CheckMouseInput: the self is not a WireBehaviour")?; g.blood_wire_check_mouse_input(w); VOID }),
    ("InstantiateObjectBehaviour.Instantiate", |g, c| { c.with(0)?; if !g.instantiate_objects.contains_key(&c.sid) { return Err("no instantiate_objects entry".into()) } g.instantiate_object(c.sid, false); VOID }),
    ("FreezeGunBehaviour.Affect", |g, c| { let i = g.effect_guns.iter().position(|x| x.id == c.sid).ok_or("no effect_guns entry")?; let col = reference(c.args.first().ok_or("no collider")?).ok_or("null collider")?; let p = g.physicals.iter().position(|p| p.colliders.contains(&col)); g.effect_gun_affect(i, p, col); VOID }),
    // OnPoolableReinitialised(pool) of the self (the generated entry passed the pool's id as the helper's).
    ("PoolableSizeHelper.OnPoolableReinitialised", |g, c| { g.poolable_size_helper_on_reinitialised(c.sid); VOID }),
    // ShouldLoadSerialisableConnections = true; the SerialisableConnectedNodes it returns are the save's (save.rs).
    ("ConnectedNodeBehaviour.OnBeforeSerialise", |g, c| { let n = c.comp("ConnectedNodeBehaviour")?; g.node_on_before_serialise(n); VOID }),
    // replay (fnlive): the live calls' real argument lists, which the generated entries miss.
    ("Utils.ContainsExpanded", |g, c| { let _ = g; let c = c.with(3)?; let a = c.arg(0)?; let b = crate::game::Bounds { center: xy(word(a, "m_Center")?)?, extents: xy(word(a, "m_Extents")?)? }; Ok(Enc::enc(b.contains_expanded(c.p_arr::<2>(1)?, c.p_f32(2)?))) }),
    ("ExplosionCreator.CreatePulseExplosion", |g, c| { let c = c.with(5)?; g.create_pulse_explosion_with(c.p_arr::<3>(0)?, c.p_f32(1)?, c.p_f32(2)?, c.p_bool(3)?, c.p_bool(4)?); VOID }),
    ("LimbBehaviour.PhysicalBehaviour_OnDisintegration", |g, c| { let l = c.with_self(1)?.p_usize(g, 0)?; g.limb_on_disintegration(l); VOID }),
    // Live (fnlive): GetFalloff(intensity, sqrDistance, max) reads self's mouseMovement; PinTool's OnSelect/OnToolChosen
    // take no args (wood: self is a WoodPinTool).
    ("AOEPowerTool.GetFalloff", |g, c| { let c = c.with(4)?; let pw = c.p_usize(g, 0)?; Ok(Enc::enc(super::super::Game::power_falloff(g.powers[pw].mouseMovement, c.p_f32(1)?, c.p_f32(2)?, c.p_f32(3)?))) }),
    // Live BleedingParticleBehaviour.Awake takes no args: FancyEffects is the preference it reads.
    ("BleedingParticleBehaviour.Awake", |g, c| { let c = c.with(1)?; let b = c.p_usize(g, 0)?; let fancy = g.prefs.FancyEffects; g.bleeding[b].awake(fancy); VOID }),
    ("PinTool.OnSelect", |g, c| { let wood = pin_wood(c)?; g.pin_on_select(wood); VOID }),
    ("PinTool.OnToolChosen", |g, c| { let wood = pin_wood(c)?; g.pin_tool_choose(wood); VOID }),
    // Live (fnlive) PhysicalBehaviour.Decal(DecalInstruction): the struct's backing fields (type a DecalDescriptor by name, as fnbigb's dec: arg).
    ("PhysicalBehaviour.Decal", |g, c| {
        let a = c.args.first().ok_or("no DecalInstruction")?;
        // The recorder since lane1b: fnbigb's "dec:<descriptor>,<globalPosition>,<size>,<colourMultiplier>".
        if let Some(d) = a.strip_prefix("dec:") {
            let v: Vec<&str> = d.split(',').collect();
            let [name, rest @ ..] = &v[..] else { return Err("dec arg".into()) };
            let [px, py, size, r, gg, b, al] = rest[..] else { return Err("dec arg length".into()) };
            let ty = if *name == "null" { crate::game::physical::NULL_DECAL } else { g.decal_descriptor(name) };
            let p = c.phys(g)?;
            g.physical_behaviour_decal(p, ty, [f(px), f(py)], f(size), [f(r), f(gg), f(b), f(al)]);
            return VOID;
        }
        let k = |n: &str| word(a, &format!("<{n}>k__BackingField"));
        let ty = match k("type")? { "null" => crate::game::physical::NULL_DECAL, t => g.decal_descriptor(t.trim_start_matches('@').rsplit_once('#').ok_or("bad decal type")?.0) };
        let p = c.phys(g)?;
        g.physical_behaviour_decal(p, ty, xy(k("globalPosition")?)?, f(k("size")?), floats(k("colourMultiplier")?));
        VOID
    }),
    // Live (fnlive) SyringeBehaviour.Lodged(Stabbing) and Dislodged(Penetration): self is the syringe.
    ("SyringeBehaviour.Lodged", |g, c| { let a = c.args.first().ok_or("no Stabbing")?; let sy = g.syringes.iter().position(|x| x.id == c.sid).ok_or("no syringe")?; let v = phys_by_id(g, reference(word(a, "victim")?).ok_or("null victim")?)?; g.syringe_lodged(sy, v); VOID }),
    ("SyringeBehaviour.Dislodged", |g, c| { let sy = g.syringes.iter().position(|x| x.id == c.sid).ok_or("no syringe")?; let pen = c.penetration(g, 0)?; g.syringe_dislodged(sy, pen); VOID }),
    ("SkinMaterialHandler.Stabbed", |g, c| { let a = c.args.first().ok_or("no Stabbing")?; let l = c.comp("SkinMaterialHandler")?; let st = c.record_index(g, reference(word(a, "stabber")?).ok_or("null stabber")?)?.1; g.skin_stabbed(l, st, xy(word(a, "point")?)?); VOID }),
    ("LimbBehaviour.MoveIntoPose", |g, c| { let w = c.with(4)?; let l = w.p_usize(g, 0)?; let p = pose_arg(g, g.limbs[l].Person, w.arg(1)?)?; g.move_into_pose(l, p, w.p_f32(2)?, w.p_f32(3)?); VOID }),
    ("LimbBehaviour.MoveIntoPoseAt", |g, c| { let w = c.with(4)?; let l = w.p_usize(g, 0)?; let p = pose_arg(g, g.limbs[l].Person, w.arg(1)?)?; g.move_into_pose_at(l, p, w.p_f32(2)?, w.p_f32(3)?); VOID }),
    ("PersonBehaviour.SetPose", |g, c| { let w = c.with(2)?; let person = w.p_usize(g, 0)?; let p = pose_arg(g, person, w.arg(1)?)?; g.set_pose(person, p); VOID }),
    ("CameraListenerBehaviour.get_NormalisedDistance", |g, c| { c.with(0)?; Ok(Enc::enc(g.camera.normalised_distance)) }),
    // fntiny: GetRayDir(initial, index) on the entity (the generated entry passed RayAngle/t as the game's args).
    ("PyroPowerToolEntity.GetRayDir", |g, c| { let c = c.with(3)?; let e = c.p_usize(g, 0)?; let init = c.p_arr::<2>(1)?; let k: i32 = c.p_int(2)?; let x = &g.pyro_entities[e]; Ok(Enc::enc(super::super::Game::pyro_ray_dir(init, x.RayAngle, x.RayCount as i32, k))) }),
    // fntiny r2: GetRandomPosInCone(initial, maxRange) on the entity (its RayAngle), from the recorded Random state.
    ("PyroPowerToolEntity.GetRandomPosInCone", |g, c| { let c = c.with(3)?; let e = c.p_usize(g, 0)?; let init = c.p_arr::<2>(1)?; let range = c.p_f32(2)?; let a = g.pyro_entities[e].RayAngle; Ok(Enc::enc(g.pyro_random_pos_in_cone(a, init, range))) }),
    // MapConfig with its own argument types (fnmaped, tools/recorder/MapConfigProbe.cs): EnvironmentalSettings "env:..",
    // OverlayState "overlay:<slot>". SetGravity reads only settings.Gravity.
    ("MapConfig.ApplySettings", |g, c| { let s = crate::game::map::check::parse_settings(c.args.first().ok_or("no argument")?)?; g.map_config_apply_settings(s); VOID }),
    ("MapConfig.SetGravity", |g, c| { let s = crate::game::map::check::parse_settings(c.args.first().ok_or("no argument")?)?; g.map_config_set_gravity(s.Gravity); VOID }),
    ("MapConfig.SetFloodLights", |g, c| { let s = crate::game::map::check::parse_settings(c.args.first().ok_or("no argument")?)?; g.map_config_set_flood_lights(&s); VOID }),
    // Start = Reinitialise; Reinitialise and PrepareLightningPrefab with the live MapConfig's Start data.
    // Liquid.GetDisplayName overrides, on the case's registry liquid.
    ("TissueDeconstructionAgent.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("WaterBreathingSerum.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("MendingSerum.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("MuscleEnhancementSerum.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("OsteomorphosisAgent.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("RegenerationSerum.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("ZombiePoisonLiquid.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("UltraStrengthSyringe.GetLiquidID", |g, c| { let _ = g; let n = c.s.of("FnCase").next().ok_or("no FnCase")?.name.clone(); Ok(Enc::enc(super::super::syringe::liquid_id(n.split('.').next().unwrap_or("")))) }),
    ("WaterBreathingSyringe.GetLiquidID", |g, c| { let _ = g; let n = c.s.of("FnCase").next().ok_or("no FnCase")?.name.clone(); Ok(Enc::enc(super::super::syringe::liquid_id(n.split('.').next().unwrap_or("")))) }),
    ("ZombieSyringe.GetLiquidID", |g, c| { let _ = g; let n = c.s.of("FnCase").next().ok_or("no FnCase")?.name.clone(); Ok(Enc::enc(super::super::syringe::liquid_id(n.split('.').next().unwrap_or("")))) }),
    ("EmptySyringe.GetLiquidID", |g, c| { let _ = g; let n = c.s.of("FnCase").next().ok_or("no FnCase")?.name.clone(); Ok(Enc::enc(super::super::syringe::liquid_id(n.split('.').next().unwrap_or("")))) }),
    ("PinkSyringe.GetLiquidID", |g, c| { let _ = g; let n = c.s.of("FnCase").next().ok_or("no FnCase")?.name.clone(); Ok(Enc::enc(super::super::syringe::liquid_id(n.split('.').next().unwrap_or("")))) }),
    ("AcidLiquid.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("AdrenalineLiquid.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("BeverageM04.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("BoneHurtingJuiceLiquid.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("CoagulationLiquid.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("CrushingPoison.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("DebugDiscolorationLiquid.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("ExoticLiquid.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("ExplosionPoison.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("FreezePoisonLiquid.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("InertLiquid.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("InstantDeathPoisonLiquid.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("KnockoutPoisonLiquid.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("LifeSerumLiquid.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("MassManipulationPoison.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("MusclePoison.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("Nitroglycerine.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("NumbingPoison.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("Oil.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("PainKillerLiquid.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("PinkDormantLiquid.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("ReflectionPoison.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("SizeManipulationPoison.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("TransparencyPoison.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("Tritium.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("UltraStrengthSerumLiquid.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("VestibularPoison.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    // gapC r1: the rest of the registry liquids (recorded as PI cases on the registry liquid, as the ones above).
    ("Blood.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("CirculationPoison.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("CombustionAgent.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("Coolant.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("DurabilitySerum.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("GorseBlood.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    ("ImmortalitySerum.GetDisplayName", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::liquid_display_name(c.liq()?))) }),
    // Liquid.OnUpdate overrides, through Liquid.OnUpdate's virtual call on the case's registry liquid.
    ("WaterBreathingSerum.OnUpdate", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.liquid_on_update(a0, a1); VOID }),
    ("WindControllerBehaviour.Update", |g, c| { let i = g.wind_controllers.iter().position(|x| x.id == c.sid).ok_or("no WindControllerBehaviour")?; g.wind_controller_update(i); VOID }),
    ("SlightMovement.Update", |g, c| { let i = g.slight_movements.iter().position(|x| x.id == c.sid).ok_or("no SlightMovement")?; g.slight_movement_update(i); VOID }),
    ("SentryTurretGunBehaviour.get_BarrelPosition", |g, c| { let i = g.sentry_guns.iter().position(|x| x.id == c.sid).ok_or("no SentryTurretGunBehaviour")?; let f = g.sentry_guns[i].firearm; Ok(Enc::enc(g.barrel_position(f))) }),
    ("KnockoutPoisonLiquid.OnUpdate", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.liquid_on_update(a0, a1); VOID }),
    ("FreezePoisonLiquid.OnUpdate", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.liquid_on_update(a0, a1); VOID }),
    ("BoneHurtingJuiceLiquid.OnUpdate", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.liquid_on_update(a0, a1); VOID }),
    ("CoagulationLiquid.OnUpdate", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.liquid_on_update(a0, a1); VOID }),
    ("InstantDeathPoisonLiquid.OnUpdate", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.liquid_on_update(a0, a1); VOID }),
    ("PainKillerLiquid.OnUpdate", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.liquid_on_update(a0, a1); VOID }),
    // batch4 r9: the base body on a registered subclass that inherits it (GenRun picks one per case: FnCase liquid=).
    ("TemporaryBodyLiquid.OnUpdate", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.liquid_on_update(a0, a1); VOID }),
    ("AdrenalineLiquid.OnUpdate", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.liquid_on_update(a0, a1); VOID }),
    ("Oil.OnUpdate", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.liquid_on_update(a0, a1); VOID }),
    ("DebugDiscolorationLiquid.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("PointLiquidTransferBehaviour.GetPoint", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; Ok(Enc::enc(g.point_liquid_get_point(a0))) }),
    ("PointLiquidTransferBehaviour.Transfer", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_f32(1)?; g.point_liquid_transfer(a0, a1); VOID }),
    ("LiquidCanisterBehaviour.OnLiquidEnter", |g, c| { let o = c.with(2)?.p_owner(g, 0)?; g.on_liquid_enter(o, c.liquid(0)?); VOID }),
    ("LiquidCanisterBehaviour.OnLiquidExit", |g, c| { let o = c.with(2)?.p_owner(g, 0)?; g.on_liquid_exit(o, c.liquid(0)?); VOID }),
    ("LiquidCanisterBehaviour.Start", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; g.liquid_canister_update_text(a0); VOID }),
    ("LiquidCanisterBehaviour.get_Limits", |g, c| { let o = c.with(1)?.p_owner(g, 0)?; Ok(Enc::enc(g.container_limits(o))) }),
    ("LiquidPumpBehaviour.get_Pressure", |g, c| { let o = c.with(1)?.p_owner(g, 0)?; Ok(Enc::enc(g.container_pressure(o))) }),
    ("LiquidContainerController.OnWillRenderObject", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; g.liquid_container_will_render(a0); VOID }),
    // OnBecameVisible: SetShaderProperties.
    ("LiquidContainerController.OnBecameVisible", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; g.liquid_set_shader_properties(a0); VOID }),
    ("LiquidContainerController.get_Color", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; Ok(vec(g.liquid_containers[a0].color)) }),
    ("LiquidContainerHoleBehaviour.IsParticleSubmerged", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; let w = g.liquid_holes[a0].water_puff; Ok(Enc::enc(g.water_puff_submerged(w))) }),
    // OnEnable: SetMode(Mode).
    ("LiquidPumpBehaviour.OnEnable", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; let m = g.liquid_pumps[a0].Mode; g.liquid_pump_set_mode(a0, m); VOID }),
    ("LifeSerumLiquid.GeneralHealing", |g, c| { c.liq()?; g.life_serum_general_healing(c.arg_index(0, "LimbBehaviour")?); VOID }),
    // Liquid's registry statics: the registry is the LIQUIDS table, identity first.
    ("Liquid.GetIdentity", |g, c| { let _ = g; Ok(Enc::enc(super::super::circulation::LIQUIDS[c.liquid(0)?].0)) }),
    ("Liquid.HasLiquid", |g, c| { let _ = g; Ok(Enc::enc(c.args.first() != Some(&"null") && c.liquid(0).is_ok())) }),
    // LiquidLevelGateBehaviour.OnContinuousUpdate: passingThrough is never set (FixedUpdate throws first), so it sends nothing.
    ("LiquidLevelGateBehaviour.OnContinuousUpdate", |g, c| { let _ = (g, c); VOID }),
    // gapA r20 menu compare: these Starts are the whole menu_start / translated_menu (their buttons, then the rest), as the live path runs them.
    ("HovercarBehaviour.Start", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; g.menu_start(crate::game::misc::Menu::Hovercar(a0)); VOID }),
    ("JukeboxBehaviour.Start", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; g.menu_start(crate::game::misc::Menu::Jukebox(a0)); VOID }),
    ("DisplayBehaviour.Start", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; g.menu_start(crate::game::misc::Menu::Display(a0)); VOID }),
    // gapC r24: EditableTintBehaviour.Start on its editable_tints slot and Awake's phys (the PhysicalBehaviour beside it): the button.
    ("EditableTintBehaviour.Start", |g, c| {
        let i = match g.script_slots.get(&c.sid) { Some(&super::super::save_slots::Slot::EditableTints(i)) => i, _ => return Err(format!("no EditableTint slot #{}", c.sid)) };
        let p = phys_by_id(g, c.sibling_id(c.sid, "PhysicalBehaviour")?.ok_or("no PhysicalBehaviour beside the EditableTint")?)?;
        g.menu_start(crate::game::misc::Menu::EditableTint(i, p));
        VOID
    }),
    ("MagnetBehaviour.Start", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; g.magnet_behaviour_start(a0); g.translated_menu(crate::game::translated::TBehaviour::MagnetBehaviour(a0)); VOID }),
    // gapA r21: the other translated Starts with buttons, as the live PendingStart::Translated path (tr_start, then translated_menu).
    // gapA r22: the whole OnDeselect (the drag joint's Destroy and the mass too), not only its pause-mode loop.
    ("DragTool.OnDeselect", |g, c| { c.with(0)?; g.drag_on_deselect(); VOID }),
    ("BoatMotorBehaviour.Start", |g, c| { let c = c.with(1)?; let b = crate::game::translated::TBehaviour::BoatMotorBehaviour(c.p_usize(g, 0)?); g.tr_start(b); g.translated_menu(b); VOID }),
    ("MotorisedHingeBehaviour.Start", |g, c| { let c = c.with(1)?; let b = crate::game::translated::TBehaviour::MotorisedHingeBehaviour(c.p_usize(g, 0)?); g.tr_start(b); g.translated_menu(b); VOID }),
    ("RotorBehaviour.Start", |g, c| { let c = c.with(1)?; let b = crate::game::translated::TBehaviour::RotorBehaviour(c.p_usize(g, 0)?); g.tr_start(b); g.translated_menu(b); VOID }),
    // WaterImpactBehaviour.Shot on its GameObject's bare collider (Splash is that collider's water_impact).
    ("WaterImpactBehaviour.Shot", |g, c| {
        let col = *g.water_impact_colliders.get(&c.sid).ok_or_else(|| format!("no WaterImpactBehaviour #{}", c.sid))?;
        let splash = g.bare_colliders[&col].water_impact.ok_or("no Splash")?;
        let c = c.with_shot(1)?;
        let (n, p, d) = (c.c.shot::<[f32; 2]>("normal", 0)?, c.c.shot::<[f32; 2]>("point", 0)?, c.c.shot::<f32>("damage", 0)?);
        g.water_impact_shot(splash, n, p, d);
        VOID
    }),
    // fnmaped: the stab methods on the knife the recorder lodges (MapConfigProbe.cs StabCase), self its PhysicalBehaviour.
    ("PhysicalBehaviour.UndoPenetrationNoCollision", |g, c| { g.undo_penetration_no_collision(c.penetration(g, 0)?); VOID }),
    ("PhysicalBehaviour.HandleStabRelease", |g, c| { let p = c.phys(g)?; g.handle_stab_release(p, c.penetration(g, 0)?); VOID }),
    ("PhysicalBehaviour.DestroyStabJoint", |g, c| { g.destroy_stab_joint(c.penetration(g, 0)?); VOID }),
    ("PhysicalBehaviour.OnJointBreak2D", |g, c| { let p = c.phys(g)?; g.physical_on_joint_break_slider(p, c.adopted_joint(g, 0)); VOID }),
    ("PhysicalBehaviour.TryGetFreeCollisionBufferIndex", |g, c| { let _ = (g, c); let (r, i) = crate::game::physical::try_get_free_collision_buffer_index(); Ok(format!("{} out={i}", bit(r))) }),
    // SerialisedLayerBehaviour on the layer editor's component (self=): its object's PhysicalBehaviour.
    ("SerialisedLayerBehaviour.Awake", |g, c| { g.serialised_layer_awake(c.layer_self(g)?); VOID }),
    ("SerialisedLayerBehaviour.Start", |g, c| { g.serialised_layer_start(c.layer_self(g)?); VOID }),
    ("SerialisedLayerBehaviour.SetOrder", |g, c| { g.serialised_layer_set_order(c.layer_self(g)?, c.n(0)?, c.n(1)?); VOID }),
    ("SerialisedLayerBehaviour.MoveLayer", |g, c| { g.serialised_layer_move_layer(c.layer_self(g)?, c.n(0)?); VOID }),
    ("SerialisedLayerBehaviour.SendToBack", |g, c| { g.serialised_layer_send_to_back(c.layer_self(g)?); VOID }),
    ("SerialisedLayerBehaviour.BringToFront", |g, c| { g.serialised_layer_bring_to_front(c.layer_self(g)?); VOID }),
    ("SerialisedLayerBehaviour.FindTopmostAround", |g, c| { let r = g.serialised_layer_find_topmost_around(c.v(0), c.n(1)?); Ok(super::fnmaped::find_around(g, &c.args, r, i32::MIN)) }),
    ("SerialisedLayerBehaviour.FindBottommostAround", |g, c| { let r = g.serialised_layer_find_bottommost_around(c.v(0), c.n(1)?); Ok(super::fnmaped::find_around(g, &c.args, r, i32::MAX)) }),
    // fnmaped round 11: the Penetration getters on the lodged knife's live penetration (FnCase id the knife).
    ("Penetration.GetCurrentDepth", |g, c| { let i = super::fnmaped::live_penetration(g, c.phys(g)?)?; Ok(Enc::enc(g.penetration_current_depth(i))) }),
    ("Penetration.GetDirection", |g, c| { let i = super::fnmaped::live_penetration(g, c.phys(g)?)?; Ok(Enc::enc(g.penetration_direction(i))) }),
    ("Penetration.GetPoint", |g, c| { let i = super::fnmaped::live_penetration(g, c.phys(g)?)?; Ok(Enc::enc(g.penetration_point(i))) }),
    ("PhysicalBehaviour.GetSharpEnd", |g, c| { let a = super::fnmaped::sharp_axis(c.args.first().ok_or("no SharpAxis")?)?; Ok(Enc::enc(g.get_sharp_end(c.phys(g)?, &a))) }),
    // fnmaped round 12: HandleStabbing and DoStab on a knife set against a limb (MapConfigProbe.PokeCase), IsInsideOf on a
    // limb's own non-trigger colliders.
    // v52h r4: the private previousVelocity HandleStabbing reads, from the recorder's " stpre=pv:<x>,<y>,<victim x>,<victim y>" (FnTiny.R4Of; the victim pair, "-" without one, is informational: the victim's PhysicalBehaviour record already carries its previousVelocity); a recording
    // from before it (the 1.28.41 fnmaped ones) leaves the loaded value.
    ("PhysicalBehaviour.HandleStabbing", |g, c| { let col = c.with(3)?.p_collision(1)?; let ct = super::fnmaped::contact_point(c.args.get(1).ok_or("no ContactPoint2D")?)?; let p = c.phys(g)?; if crate::assets::il::is_1_28_45() { super::all_property_clips(g)?; } if let Some(v) = st_pre(c).and_then(|v| v.strip_prefix("pv:")) { let w: Vec<f32> = v.split(',').filter_map(|x| u32::from_str_radix(x, 16).ok()).map(f32::from_bits).collect(); g.physicals[p].previousVelocity = [w[0], w[1]]; } let r = g.handle_stabbing(p, &col, &ct); if crate::assets::il::is_1_28_45() { Ok(bit(r)) } else { VOID } }),
    ("PhysicalBehaviour.DoStab", |g, c| { c.with(4)?; let p = c.phys(g)?; let other = c.arg_phys(g, 0)?; let a = super::fnmaped::sharp_axis(c.args[1])?; let pen = super::fnmaped::fresh_penetration(g, c.args[2])?; g.do_stab(p, other, a, pen); VOID }),
    // fnmaped r15: MapConfigProbe.Seed sets SpawnSpawnParticles on even cases, off on odd ones (the PI record holds it).
    ("PhysicalBehaviour.SetupSpawnPrefab", |g, c| { c.with(1)?; let p = c.phys(g)?; g.physical_setup_spawn_prefab(p); VOID }),
    ("PhysicalBehaviour.SetupBubblePrefab", |g, c| { c.with(1)?; let p = c.phys(g)?; g.physical_setup_bubble_prefab(p); VOID }),
    ("PhysicalBehaviour.SetupChargedPrefab", |g, c| { c.with(1)?; let p = c.phys(g)?; g.physical_setup_charged_prefab(p); VOID }),
    ("PhysicalBehaviour.SetupSmokePrefab", |g, c| { c.with(1)?; let p = c.phys(g)?; g.physical_setup_smoke_prefab(p); VOID }),
    ("PhysicalBehaviour.IsInsideOf", |g, c| { c.with(3)?; let p = c.phys(g)?; let colls = super::fnmaped::own_colliders(g, p, c.args[0])?; Ok(Enc::enc(g.is_inside_of(p, &colls, c.v(1)))) }),
    // Ignite(bool), Ignite(bool, Vector2), Ignite(Vector2): by the args.
    ("PhysicalBehaviour.Ignite", |g, c| {
        let p = c.phys(g)?;
        match c.args.len() {
            2 => g.ignite_at(p, c.b(0), c.v(1)),
            1 if c.args[0].contains(',') => g.ignite_at(p, false, c.v(0)),
            1 => g.ignite(p, c.b(0)),
            n => return Err(format!("Ignite with {n} args")),
        }
        VOID
    }),
    // CreateImpactEffect(Shot, size) is CreateImpactEffect(shot.point, shot.normal, size).
    ("PhysicalBehaviour.CreateImpactEffect", |g, c| {
        let p = c.phys(g)?;
        if c.args.len() == 2 {
            let (pt, n) = (c.shot::<[f32; 2]>("point", 0)?, c.shot::<[f32; 2]>("normal", 0)?);
            g.create_impact_effect(p, pt, n, c.f(1));
        } else {
            g.create_impact_effect(p, c.v(0), c.v(1), c.f(2));
        }
        VOID
    }),
    ("WaterBehaviour.GetSubmergedObjects", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; Ok(Enc::enc(g.water_submerged_objects(a0))) }),
    ("WaterBehaviour.SetEvaporated", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; g.water_set_evaporated(a0); VOID }),
    ("WaterBehaviour.OnTriggerExit2D", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_int(1)?; g.water_on_trigger(a0, a1); VOID }),
    ("MapConfig.Start", |g, c| { let _ = c; let m = g.map_config.clone().ok_or("no MapConfig")?; g.map_config_start(&m); VOID }),
    ("MapConfig.Reinitialise", |g, c| { let _ = c; let m = g.map_config.clone().ok_or("no MapConfig")?; g.map_config_reinitialise(&m); VOID }),
    ("MapConfig.PrepareLightningPrefab", |g, c| { let _ = c; let m = g.map_config.clone().ok_or("no MapConfig")?; g.map_config_prepare_lightning_prefab(&m); VOID }),
    ("MapConfig.OverlayFadeStep", |g, c| { let k = crate::game::map::check::parse_overlay(c.args.first().ok_or("no argument")?)?; g.map_config_overlay_fade_step(k); VOID }),
    // DoSplash(point, size): the size is the particles' shape radius only (presentation).
    ("LavaBehaviour.DoSplash", |g, c| { let c = c.with(3)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_arr::<2>(1)?; g.lava_do_splash(a0, [a1[0], a1[1], 0.0]); VOID }),
    // MapLightBehaviour's coroutines: the call runs to their WaitForSeconds (the generated entries return void).
    ("MapLightBehaviour.DelayedActivation", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_int(1)?; Ok(format!("step:WaitForSeconds({})", hex(g.map_light_delayed_activation(a0, a1)))) }),
    ("MapLightBehaviour.DelayedDeactivation", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_int(1)?; Ok(format!("step:WaitForSeconds({})", hex(g.map_light_delayed_deactivation(a0, a1)))) }),
    // MapLightBehaviour.Start: Activated = StartEnabled (static), UpdateActivated(!StartEnabled), StartCoroutine(DelayedActivation()).
    ("MapLightBehaviour.Start", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; g.map_light_start(a0); VOID }),
    // FireBolt is a coroutine: its first MoveNext ends (no hit) or yields the first flash's WaitForSeconds(0.032f).
    ("LightningToolEntityBehaviour.FireBolt", |g, c| {
        let i = g.lightning_entities.iter().position(|x| x.id == c.id).ok_or("no lightning_entities entry")?;
        let seq = g.coroutine_seq;
        g.lightning_fire_bolt(i);
        Ok(match g.coroutines.iter().find(|x| x.seq > seq && x.owner == c.id) {
            Some(_) => format!("step:WaitForSeconds({})", hex(0.032f32)),
            None => "step:end".into(),
        })
    }),
    // DoAtTimeOnce.StartSequence is a coroutine: its first MoveNext yields the first enabled action's WaitForSeconds(Delay), or ends.
    ("DoAtTimeOnce.StartSequence", |g, c| {
        let d = g.do_at_time_onces.iter().position(|x| x.id == c.id).ok_or("no do_at_time_onces entry")?;
        let first = g.do_at_time_onces[d].actions.iter().find(|a| a.Enabled).map(|a| a.Delay);
        g.do_at_time_once_next(d, 0);
        Ok(match first {
            Some(w) => format!("step:WaitForSeconds({})", hex(w)),
            None => "step:end".into(),
        })
    }),
    // The getter returns powerOutput (the map paired it with Start).
    ("ReactorCoreBehaviour.get_PowerOutput", |g, c| { let i = g.reactor_cores.iter().position(|x| x.id == c.id).ok_or("no reactor_cores entry")?; Ok(Enc::enc(g.reactor_cores[i].powerOutput)) }),
    // EnergyVesselLightningBehaviour.Init is a coroutine: the call runs to its first yield (the line off, the random wait).
    ("EnergyVesselLightningBehaviour.Init", |g, c| { let i = g.energy_bolts.iter().position(|x| x.id == c.id).ok_or("no energy_bolts entry")?; let line = g.energy_bolts[i].line; g.lines[line].enabled = false; let w = g.random.range(0.1, 1.0); g.start_wait(c.id, w, crate::game::destroyable::Step::EnergyBoltInit(i)); Ok(format!("step:WaitForSeconds({:08x})", w.to_bits())) }),
    // A map conduit set has no record of its own (the generated entry's record_index has none): found by id.
    ("ReactorConduitSetBehaviour.GetStatus", |g, c| { let i = g.reactor_conduit_sets.iter().position(|x| x.id == c.id).ok_or("no reactor_conduit_sets entry")?; Ok(g.reactor_conduit_set_status(i).to_string()) }),
    // fnweap: HandleSounds(Collision2D, float) reads only the impulse; the rewrite takes the impulse alone.
    ("PhysicalBehaviour.HandleSounds", |g, c| { let c = c.with(3)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_f32(2)?; g.handle_sounds(a0, a1); VOID }),
    // fnweap: UseWireBehaviour.IsolatedActivation / IsolatedContinuousActivation(signal): the recorder's signal has a Path, so Use(activation) /
    // UseContinuous(activation): use_wire_receive on a fresh direct channel-0 signal.
    ("UseWireBehaviour.IsolatedActivation", |g, c| signal_entry(g, c, |g, w, a| g.use_wire_receive(w, a, false))),
    ("UseWireBehaviour.IsolatedContinuousActivation", |g, c| signal_entry(g, c, |g, w, a| g.use_wire_receive(w, a, true))),
    // fnweap: Use / UseContinuous with the recorder's signal (direct, channel 0); ContinuousActivationBehaviour.AssertState only reads.
    ("UseWireBehaviour.Use", |g, c| signal_entry(g, c, |g, w, a| g.use_wire_receive(w, a, false))),
    ("UseWireBehaviour.UseContinuous", |g, c| signal_entry(g, c, |g, w, a| g.use_wire_receive(w, a, true))),
    // fnweap: the coroutines' first MoveNext, on the recorder's signal itself.
    ("UseWireBehaviour.SendUseMessage", |g, c| { let (w, a) = use_wire_signal(g, c)?; let mut r = g.use_wire_send_routine(w, a, false); Ok(routine_step(g, &mut r)) }),
    ("UseWireBehaviour.SendUseContinuousMessage", |g, c| { let (w, a) = use_wire_signal(g, c)?; let mut r = g.use_wire_send_routine(w, a, true); Ok(routine_step(g, &mut r)) }),
    // fnweap: UseWireBehaviour.Tick: base.Tick (currentDistance, read only by the save), then the burn while CanBreak.
    ("UseWireBehaviour.Tick", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; if g.blood_wires[w].CanBreak { g.wire_burn(w); } VOID }),
    // fnweap: OnContinuousUpdate(float dt) does not read dt.
    // fc41 r3: the recorder's waiting signals (" stpre=q:<toSendOut.Count>", new non-direct ones on the wire's channel), drained:
    // " st=q:" after.
    ("UseWireBehaviour.OnContinuousUpdate", |g, c| {
        let c = c.with(2)?;
        let w = c.p_usize(g, 0)?;
        let Some(pre) = st_pre(c.c) else { g.use_wire_on_continuous_update(w); return VOID };
        let n: usize = pre.strip_prefix("q:").and_then(|v| v.parse().ok()).ok_or(format!("bad q stpre {pre}"))?;
        let ch = g.blood_wires[w].Channel;
        g.blood_wires[w].toSendOut = (0..n).map(|_| g.signal_direct(false, ch)).collect();
        g.use_wire_on_continuous_update(w);
        Ok(format!(" st=q:{}", g.blood_wires[w].toSendOut.len()))
    }),
    // fnweap: WireBehaviour.GetNormalisedStress: untypedJoint.reactionForce.sqrMagnitude / (breakForce * breakForce).
    ("WireBehaviour.GetNormalisedStress", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; let f = g.blood_wires[w].joint; let r = g.fixed_joint_reaction_force(f); let bf = g.fixed_joints[f].state.break_force; Ok(Enc::enc((r[0] * r[0] + r[1] * r[1]) / (bf * bf))) }),
    // fnweap: WireBehaviour.Initialise, called again on a live wire: the Initialise part of the wires' Start, then its new InvisibilityTracker.
    ("WireBehaviour.Initialise", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.wire_initialise(w); g.tracker_new(crate::game::tool_invisibility::Owner::Wire(w)); VOID }),
    // fnweap: UseWireBehaviour.Start, called directly: the whole Start (DistanceJointWireBehaviour.Start, Initialise, the use wire's part).
    // `started` is DelayedStartCall's has-run bit, which a direct call does not test.
    ("UseWireBehaviour.Start", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.blood_wires[w].started = false; g.blood_wire_start(w); VOID }),
    // fnweap: WireBehaviour.Slice on its own wire (the generated entry passed the wire's index as a physical's).
    ("WireBehaviour.Slice", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.wire_slice(w); VOID }),
    // fnweap: WireBehaviour.OnDestroy (a wire kind that does not override it).
    ("WireBehaviour.OnDestroy", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.blood_wire_on_destroy(w); VOID }),
    // fnweap: EnergyWireBehaviour.OnDestroy called directly: base.OnDestroy (WireBehaviour's: Destroy(untypedJoint) and the rest), then its own.
    ("EnergyWireBehaviour.OnDestroy", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.blood_wire_on_destroy(w); g.energy_wire_on_destroy(w); VOID }),
    // fnweap: NewUseWireBehaviour.OnDestroy called directly: base.OnDestroy (WireBehaviour's) first, then the listener and target
    // (blood_wire_on_destroy runs the whole chain; the generated entry ran only NewUse's own part).
    ("NewUseWireBehaviour.OnDestroy", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.blood_wire_on_destroy(w); VOID }),
    // fnweap r17: EnergyWireBehaviour.SetWireColours returns the line's colours (FnWeap.NonVirtual's `lines=start;end`).
    // LineRenderer.startColor/endColor read back through its gradient's Color32 keys: Round(Clamp01(x) * 255) / 255.
    ("EnergyWireBehaviour.SetWireColours", |g, c| {
        let c = c.with(1)?;
        let w = c.p_usize(g, 0)?;
        g.energy_wire_set_wire_colours(w);
        let q = |v: [f32; 4]| v.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() / 255.0);
        Ok(format!("lines={};{}", vec(q(g.blood_wires[w].start_color)), vec(q(g.blood_wires[w].end_color))))
    }),
    ("SliderJointWireBehaviour.GetPoints", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; Ok(Enc::enc(g.slider_wire_get_points(g.blood_wires[w].joint))) }),
    ("PhaseLinkBehaviour.OnDestroy", |g, c| { let l = g.phase_links.iter().position(|x| x.id == c.sid).ok_or("no phase link")?; g.phase_link_on_destroy(l); VOID }),
    // LinkDeviceBehaviour.OnDestroy: the base body alone (FnWeap.NonVirtual on the PhaseLinkBehaviour).
    // LinkDeviceBehaviour.PrepareSpriteRendererOrder(phys, container): FnWeap's fresh container (its Clone record's local
    // transform) under a limb; the ret is FnWeap.NonVirtual's (the new SpriteRenderer, then the ExistInDetailView).
    ("LinkDeviceBehaviour.PrepareSpriteRendererOrder", |g, c| {
        let w = c.with(2)?;
        let p = c.arg_phys(g, 0)?;
        let id = reference(w.arg(1)?).ok_or("null container")?;
        let o = c.s.of("Clone").find(|r| r.id == id).ok_or(format!("no Clone record of container #{id}"))?;
        let (t, q, s): ([f32; 3], [f32; 4], [f32; 3]) = (floats(o.req("lpos")?), floats(o.req("lrot")?), floats(o.req("lscale")?));
        let local = crate::unity::transform::Trs { t: [t[0], t[1], t[2], 0.0], q, s: [s[0], s[1], s[2], 0.0] };
        let sr = g.allocate_instance_id();
        let r = g.link_prepare_sprite_renderer_order(p, local, sr);
        let ex = g.allocate_instance_id();
        let (sprite, _, order) = g.renderers[r].sprite;
        let name = g.assets.as_ref().unwrap().sprite(sprite)?.name;
        Ok(format!("sr={sr},layer={},order={order},sprite={name},exist={ex}", g.renderers[r].layer))
    }),
    ("LinkDeviceBehaviour.OnDestroy", |g, c| { let l = g.phase_links.iter().position(|x| x.id == c.sid).ok_or("no phase link")?; g.link_device_on_destroy(l); VOID }),
    ("AdhesiveCouplerBehaviour.StickTo", |g, c| { let c = c.with(2)?; let k = c.p_usize(g, 0)?; let col = reference(c.arg(1)?).ok_or("null collider")?; let p = g.physicals.iter().position(|p| p.colliders.contains(&col)).ok_or("no collider's physical")?; g.coupler_stick_to(k, p, col); VOID }),
    // fnweap: WireBehaviour.OnDestroy's base body alone (the generated entry took new_use_wire_on_destroy off its doc line).
    ("WireBehaviour.OnDestroy", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.wire_base_on_destroy(w); VOID }),
    // fnweap: UseWireTool.CreateWire / OnJointCreate on an existing wire's DistanceJoint2D (its `Game::fixed_joints` entry); CreateWire
    // returns the new UseWireBehaviour.
    ("UseWireTool.CreateWire", |g, c| { c.with(1)?; let f = use_wire_joint(g, c)?; let w = g.use_wire_tool_create_wire(f); Ok(format!("@{}#{}", g.physicals[g.blood_wires[w].physical].name, g.blood_wires[w].id)) }),
    ("UseWireTool.OnJointCreate", |g, c| { c.with(1)?; let f = use_wire_joint(g, c)?; let name = &c.s.of("FnCase").next().ok_or("no FnCase")?.name; let ty = name.split('.').next().unwrap(); let kind = super::super::blood_wire::WireKind::of_tool(ty).ok_or(format!("not a Use wire tool: {ty}"))?; g.use_wire_tool_on_joint_create(f, kind); VOID }),
    // fnweap: PinTool.GetPinPos with the recorder's snapToCenter and firstFound/secondFound (its FnWeapPin record).
    ("PinTool.GetPinPos", |g, c| {
        c.with(0)?;
        let r = c.s.of("FnWeapPin").next().ok_or("no FnWeapPin record (a recording before fnw17)")?;
        let found = |k: &str| r.get(k).and_then(reference).and_then(|id| g.physicals.iter().position(|p| p.id == id));
        let (first, second) = (found("firstFound"), found("secondFound"));
        g.input.controls.insert("snapToCenter".into(), crate::game::tool::Control { down: false, held: r.req("held")? == "1", up: false });
        Ok(vec(g.pin_tool_get_pin_pos(first, second)))
    }),
    // fnweap r23: PinTool.GetFoundBodies at the recorder's mouse (FnWeap.PreCase); the recorder returns firstFound;secondFound.
    ("PinTool.GetFoundBodies", |g, c| {
        c.with(0)?;
        let mouse = g.MousePosition;
        let (a, b) = g.pin_found_bodies(mouse);
        let r = |p: Option<usize>| p.map_or("null".to_string(), |p| format!("@{}#{}", g.physicals[p].name.replace(' ', "_"), g.physicals[p].id));
        Ok(format!("{};{}", r(a), r(b)))
    }),
    ("PinTool.GetBreakingThreshold", |_, c| { c.with(0)?; Ok(Enc::enc(Game::pin_tool_breaking_threshold(false))) }),
    // fnweap r23: PinTool.CreatePin(anchor, a, b, breakThreshold) on the PinTool (not WoodPinTool).
    ("PinTool.CreatePin", |g, c| { let w = c.with(4)?; let (an, a, b, t) = (w.p_arr::<2>(0)?, w.p_usize(g, 1)?, w.p_opt_usize(g, 2)?, w.p_f32(3)?); g.create_pin(an, a, b, t, false); VOID }),
    // fnweap r23: PinBehaviour.OnJointBreak2D(broken): the arg's joint by its adopted entry (a limb's own hinge is none).
    ("PinBehaviour.OnJointBreak2D", |g, c| { let p = c.with(2)?.p_usize(g, 0)?; if let Some(f) = c.adopted_joint(g, 0) { g.pin_on_joint_break_one(p, f); } VOID }),
    // fnweap: MachineGunBehaviour's BallisticsEmitter callbacks: the CallbackParams' Position and Direction.
    ("MachineGunBehaviour.OnEntry", |g, c| { let m = c.with(2)?.p_usize(g, 0)?; let (p, d) = callback_params(c.args[0])?; g.machine_gun_on_entry(m, p, d); VOID }),
    ("MachineGunBehaviour.OnExit", |g, c| { let m = c.with(2)?.p_usize(g, 0)?; let (p, d) = callback_params(c.args[0])?; g.machine_gun_on_exit(m, p, d); VOID }),
    ("MachineGunBehaviour.OnRicochet", |g, c| { let m = c.with(2)?.p_usize(g, 0)?; let (p, _) = callback_params(c.args[0])?; g.machine_gun_on_ricochet(m, p); VOID }),
    // gapB r1: OnHit(CallbackParams): the HitObject collider's id and the Position (FnTiny connects the attachment in every case).
    ("IncendiaryAttachmentBehaviour.OnHit", |g, c| { let a = attachment_of(g, c)?; let (p, _) = callback_params(c.args[0])?; let hit = arg_field(c.args[0], "HitObject").and_then(reference).ok_or("CallbackParams without HitObject")?; g.firearm_attachment_on_hit(a, hit, p); VOID }),
    // gapB r1: the OnDisintegration handlers (sender, EventArgs.Empty): the rewrite takes the subscribed component.
    ("ActOnDisintegrate.OnDisintegration", |g, c| { c.with_self(1)?; let i = g.act_on_disintegrates.iter().position(|x| x.id == c.sid).ok_or("no act_on_disintegrates entry")?; g.act_on_disintegrate_on_disintegration(i); VOID }),
    // gapA r15: the generator's arity/overload drops (fncheck_drop.txt "arity: the rewrite takes N, the game passed M"): hand entries.
    ("GoreStringBehaviour.Start", |g, c| { c.with(0)?; let r = c.s.records.iter().find(|r| r.id == c.sid && r.ty != "FnCase").ok_or("no GoreStringBehaviour record")?; let on = r.get("CreateJointOnStart") == Some("1"); g.gore_string_start(c.gore_limb()?, on); VOID }),
    ("DragTool.OnContinuousUpdate", |g, c| { c.with(1)?; g.drag_continuous_update(); VOID }),
    ("CameraShakeBehaviour.Shake", |g, c| { let w = c.with(3)?; g.camera_shake_at(w.p_f32(0)?, w.p_arr::<2>(1)?, w.p_f32(2)?); VOID }),
    // Both overloads under one name: (Vector2, float) and (int, int, float).
    ("AmbientTemperatureGridBehaviour.ChangeTemperatureAt", |g, c| { if c.args.len() == 2 { let w = c.with(2)?; g.ChangeTemperatureAtPoint(w.p_arr::<2>(0)?, w.p_f32(1)?) } else { let w = c.with(3)?; g.ChangeTemperatureAt(w.p_int(0)?, w.p_int(1)?, w.p_f32(2)?) } VOID }),
    ("AmbientTemperatureGridBehaviour.SetTemperatureAt", |g, c| { if c.args.len() == 2 { let w = c.with(2)?; g.SetTemperatureAtPoint(w.p_arr::<2>(0)?, w.p_f32(1)?) } else { let w = c.with(3)?; g.SetTemperatureAt(w.p_int(0)?, w.p_int(1)?, w.p_f32(2)?) } VOID }),
    // Utils.SendAllChannel(Continuous)IsolatedActivation(MonoBehaviour other): other's GameObject (a limb's: its PhysicalBehaviour).
    // gapA r16: InfraredThermometer.ReadTemperatureFromRay(origin, direction): the self thermometer; returns the temperature.
    ("InfraredThermometer.ReadTemperatureFromRay", |g, c| { c.with(2)?; let i = g.infrared_thermometers.iter().position(|x| x.id == c.sid).ok_or("no infrared_thermometers entry")?; let (o, d) = (c.v::<2>(0), c.v::<2>(1)); Ok(Enc::enc(g.infrared_read_temperature_from_ray(i, o, d))) }),
    // gapA r16: ResizeHandles.set_IsHovering(value), the scene singleton's auto-property.
    // gapB r23: the scene's handles (a recording has none; the generated entry's unwrap panicked).
    // gapB r23: ModdingApi.FindSpawnable(name, gameVersion): null for a blank name or one `find` (Game::get_spawnable: by name,
    // then PriorName) misses. checker limit: a found SpawnableAsset's return is its instance id, which the rewrite has not.
    ("ModdingApi.FindSpawnable", |g, c| {
        let c = c.with(2)?;
        let n = c.p_string(0)?;
        if n.trim().is_empty() { return Ok("null".into()) }
        let db = g.assets.get_or_insert_with(|| Box::new(crate::assets::db::AssetDb::open(&crate::game_assets::find_game(None).expect("the game's data")).expect("the game's data")));
        match Game::get_spawnable(db, &n) { Err(_) => Ok("null".into()), Ok(_) => Err("checker limit: a found SpawnableAsset's instance id".into()) }
    }),
    ("ResizeHandles.CheckForDragStart", |g, c| { c.with(0)?; g.recorded_resize_handles()?; g.resize_check_for_drag_start(); VOID }),
    // fc41 r18: HandleMultiDrag reads transform.position of the scene's handles, which a recording has none of.
    ("ResizeHandles.HandleMultiDrag", |g, c| { c.with(0)?; g.recorded_resize_handles()?; g.resize_handle_multi_drag(); VOID }),
    ("ResizeHandles.set_IsHovering", |g, c| { c.with(1)?; g.resize.IsHovering = c.arg::<bool>(0)?; VOID }),
    // gapB r25: SmoothLineControllerBehaviour.CalculateVertices (UpdateLine's From/To; the vertices are its links, loaded and compared).
    ("SmoothLineControllerBehaviour.CalculateVertices", |g, c| {
        c.with(1)?;
        let Some(&(_, crate::game::Behaviour::SmoothLine(s))) = g.behaviour_of.get(&c.sid) else { return Err("no SmoothLine behaviour".into()) };
        let (from, to) = (g.smooth_lines[s].From.clone(), g.smooth_lines[s].To.clone());
        g.smooth_line_calculate_vertices(s, &from, &to);
        VOID
    }),
    // gapB r25: DetachFromParentOnStart.Start re-run: DestroyIfParentDestroyed from the self's record; a recorded detached object
    // is a root (no parent: nothing), so the load's `recorded_detached` listener path (the queued Start's) is not this call's.
    ("DetachFromParentOnStart.Start", |g, c| {
        c.with(1)?;
        let d = c.s.of("ItemScript").find(|r| r.id == c.sid).and_then(|r| r.get("DestroyIfParentDestroyed")) == Some("1");
        g.recorded_detached.remove(&c.sid);
        g.detach_from_parent_on_start(c.sid, d);
        VOID
    }),
    ("Utils.SendAllChannelIsolatedActivation", |g, c| { c.with(1)?; if let Some(p) = other_physical(g, c)? { g.send_isolated(p, false) } VOID }),
    ("Utils.SendAllChannelContinuousIsolatedActivation", |g, c| { c.with(1)?; if let Some(p) = other_physical(g, c)? { g.send_isolated(p, true) } VOID }),
    ("DisintegrationCounterBehaviour.OnChildDisintegrated", |g, c| { c.with_self(1)?; if let Some(person) = human_counter_person(g, c.sid) { g.person_on_child_disintegrated(person); return VOID; } let i = g.disintegration_counters.iter().position(|x| x.id == c.sid).ok_or("no disintegration_counters entry")?; g.disintegration_counter_on_child_disintegrated(i); VOID }),
    // gapB r1: SetWingType(WingType): the airfoil's own wing type of that Name (`WingType(Name="..." ...)`).
    ("AirfoilBehaviour.SetWingType", |g, c| {
        c.with_self(2)?;
        let a = g.airfoils.iter().position(|x| x.id == c.sid).ok_or("no airfoils entry")?;
        let name = arg_field(c.args[0], "Name").map(|v| v.trim_matches('"').replace('_', " ")).ok_or("WingType without Name")?;
        let i = g.airfoils[a].WingTypes.iter().position(|w| w.Name == name).ok_or(format!("no wing type {name}"))?;
        g.airfoil_set_wing_type(a, i);
        VOID
    }),
    // gapB r1: both overloads, (ex, doSplash) and the obsolete (waterSurfaceLevel, ex, doSplash), which only calls the first: ex's Position and Range.
    ("ExplosionCreator.CreateUnderwaterExplosionEffect", |g, c| {
        let ex = c.args.iter().find(|a| a.starts_with("ExplosionParameters(")).ok_or("no ExplosionParameters arg")?;
        let pos = floats::<3>(arg_field(ex, "Position").ok_or("ExplosionParameters without Position")?);
        let range: f32 = item_f32(arg_field(ex, "Range").ok_or("ExplosionParameters without Range")?)?;
        g.create_underwater_explosion_effect(pos, range, c.args.last() == Some(&"1"));
        // The new clone's behaviour (Recorder.cs " uw="): position, GivenTop, MaxDistanceToSurface, DoSplash, localScale.
        let u = g.underwater_explosions.last().ok_or("no underwater_explosions entry")?;
        Ok(format!(" uw={},{},{},{},{}", vec(u.position), hex(u.GivenTop), hex(u.MaxDistanceToSurface), bit(u.DoSplash), vec([u.scale; 3])))
    }),
    ("WireBehaviour.get_EdgeCollider", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; Ok(Enc::enc(g.wire_edge_collider(w))) }),
    // fnweap: SerpentineBeltBehaviour.RecalculatePointArray fills worldPointArray (void).
    ("SerpentineBeltBehaviour.RecalculatePointArray", |g, c| { let c = c.with(1)?; let l = c.p_usize(g, 0)?; g.belt_recalculate_point_array(l); VOID }),
    // fnweap: void; the points go into LocalColliderGridPoints.
    ("PhysicalBehaviour.BakeColliderGridPoints", |g, c| { let c = c.with(1)?; let p = c.p_usize(g, 0)?; let baked = g.physical_bake_collider_grid_points(p); g.physicals[p].LocalColliderGridPoints = Some(baked); VOID }),
    ("UseWireBehaviour.Slice", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.wire_slice(w); VOID }),
    ("UseWireBehaviour.JointBroken", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.wire_joint_broken(w); VOID }),
    // fnweap: ReactorA5ArcBehaviour, a map object with no snapshot record: the arena's arc by its id.
    ("ReactorA5ArcBehaviour.Start", |g, c| { let c = c.with(1)?; let a = arc_index(g, c.p_int(0)?)?; g.reactor_a5_arc_start(a); VOID }),
    ("ReactorA5ArcBehaviour.GenerateBolt", |g, c| { let c = c.with(1)?; let a = arc_index(g, c.p_int(0)?)?; g.reactor_a5_arc_generate_bolt(a); VOID }),
    ("ReactorA5ArcBehaviour.FlashRoutine", |g, c| { let c = c.with(1)?; let a = arc_index(g, c.p_int(0)?)?; g.reactor_a5_arc_flash_routine(a); Ok("step:null".into()) }),
    ("ReactorA5ArcBehaviour.Affect", |g, c| { let c = c.with(3)?; let a = arc_index(g, c.p_int(0)?)?; let col = c.p_int(1)?; let p = c.p_arr::<2>(2)?; g.reactor_a5_arc_affect(a, col, p); VOID }),
    ("ReactorA5ArcBehaviour.OnSmallArcHit", |g, c| { let c = c.with(3)?; let a = arc_index(g, c.p_int(0)?)?; let p = c.p_usize(g, 1)?; let pt = c.p_arr::<2>(2)?; g.reactor_a5_arc_small_hit(a, p, pt); VOID }),
    // fnweap: JukeboxBehaviour; a track return is the game's AudioClip ref, the self record's tracks[k].
    ("JukeboxBehaviour.SetTrackFromOffset", |g, c| { let c = c.with(2)?; let j = c.p_usize(g, 0)?; let k = g.jukebox_set_track_from_offset(j, c.p_int::<i32>(1)?); jukebox_track(&c, k) }),
    ("JukeboxBehaviour.SetTrackFromIndex", |g, c| { let c = c.with(2)?; let j = c.p_usize(g, 0)?; let k = g.jukebox_set_track_from_index(j, c.p_int::<i32>(1)?); jukebox_track(&c, k) }),
    ("JukeboxBehaviour.NextSong", |g, c| { let c = c.with(1)?; let j = c.p_usize(g, 0)?; g.jukebox_song(j, 1); VOID }),
    ("JukeboxBehaviour.PreviousSong", |g, c| { let c = c.with(1)?; let j = c.p_usize(g, 0)?; g.jukebox_song(j, -1); VOID }),
    ("JukeboxBehaviour.StopMusic", |g, c| { let c = c.with(1)?; let j = c.p_usize(g, 0)?; g.jukebox_stop_music(j); VOID }),
    ("JukeboxBehaviour.ShowPlayingNotification", |g, c| { let c = c.with(1)?; let j = c.p_usize(g, 0)?; g.jukebox_show_playing_notification(j); VOID }),
    // fnweap: PinBehaviour. OnEnable/OnDisable: SpriteChild.SetActive; OnUserDelete: empty.
    ("PinBehaviour.get_AttachedToWall", |g, c| { let c = c.with(1)?; let p = c.p_usize(g, 0)?; Ok(bit(g.pin_attached_to_wall(p))) }),
    ("PinBehaviour.SyncChildPos", |g, c| pin_state_entry(g, c, |g, p| g.pin_sync_child_pos(p))),
    ("PinBehaviour.Update", |g, c| { let c = c.with(1)?; let p = c.p_usize(g, 0)?; g.pin_update(p); VOID }),
    ("PinBehaviour.FixedUpdate", |g, c| { let c = c.with(1)?; let p = c.p_usize(g, 0)?; g.pin_fixed_update(p); VOID }),
    ("PinBehaviour.GetVisualBounds", |g, c| { let c = c.with(1)?; let p = c.p_usize(g, 0)?; let (cn, e) = g.pin_visual_bounds(p); Ok(format!("Bounds(m_Center={} m_Extents={})", vec(cn), vec(e))) }),
    ("PinBehaviour.OnUserDelete", |g, c| { c.with(1)?; VOID }),
    // weapons r22: Awake: PhysicalBehaviour = GetComponent (the same object's, unchanged). CanBeDeleted, IsMouseInsideCollider.
    ("PinBehaviour.Awake", |g, c| { c.with(1)?; VOID }),
    ("PinBehaviour.CanBeDeleted", |g, c| { let c = c.with(1)?; let p = c.p_usize(g, 0)?; Ok(bit(g.pin_can_be_deleted(p))) }),
    ("PinBehaviour.IsMouseInsideCollider", |g, c| { let c = c.with(1)?; let p = c.p_usize(g, 0)?; Ok(bit(g.pin_is_mouse_inside_collider(p))) }),
    ("PinBehaviour.OnDisable", |g, c| pin_state_entry(g, c, |g, p| g.pin_sprite_child_set_active(g.pins[p].id, false))),
    ("PinBehaviour.OnEnable", |g, c| pin_state_entry(g, c, |g, p| g.pin_sprite_child_set_active(g.pins[p].id, true))),
    ("PinBehaviour.CreateJoint", |g, c| { let c = c.with(1)?; let p = c.p_usize(g, 0)?; g.pin_create_joint(p); VOID }),
    ("PinBehaviour.Start", |g, c| { let c = c.with(1)?; let p = c.p_usize(g, 0)?; g.pin_start_run(p); VOID }),
    // PinBehaviour.OnDestroy called directly: the pin's teardown, then Destroy(Joint) and Destroy(this) (both delayed, disabled at once).
    ("PinBehaviour.OnDestroy", |g, c| { let c = c.with(1)?; let p = c.p_usize(g, 0)?; g.pin_destroy(p); if let Some(f) = g.pins[p].Joint.filter(|&f| !g.fixed_joints[f].destroyed && !g.joint_destroys.iter().any(|&(_, j)| j == f)) { g.destroy_joint_delayed(f); } g.tr_destroy_component(g.pins[p].id, 0.0); VOID }),
    ("PinBehaviour.Slice", |g, c| { let c = c.with(1)?; let p = c.p_usize(g, 0)?; g.pin_slice_one(p); VOID }),
    // fnweap: the PhaseLink's two ends (VisualDeletableDetachedToolBehaviour; Awake's GetComponent<SpriteRenderer> is the end's
    // renderer) and the link (LinkDeviceBehaviour; Awake's GetComponent<PhysicalBehaviour> is `physical`, OnUserDelete is empty).
    ("VisualDeletableDetachedToolBehaviour.Awake", |g, c| { c.with(1)?; super::fnweap::link_end(g, c.sid)?; VOID }),
    ("VisualDeletableDetachedToolBehaviour.GetVisualBounds", |g, c| { c.with(1)?; let (l, e) = super::fnweap::link_end(g, c.sid)?; let (cn, ex) = g.link_end_bounds(l, e); Ok(format!("Bounds(m_Center={} m_Extents={})", vec(cn), vec(ex))) }),
    ("VisualDeletableDetachedToolBehaviour.IsMouseInsideCollider", |g, c| { c.with(1)?; let (l, e) = super::fnweap::link_end(g, c.sid)?; Ok(bit(g.link_end_inside_collider(l, e))) }),
    ("VisualDeletableDetachedToolBehaviour.Update", |g, c| { c.with(1)?; let (l, e) = super::fnweap::link_end(g, c.sid)?; g.link_end_update(l, e); VOID }),
    ("VisualDeletableDetachedToolBehaviour.OnMouseOverlapEvent", |g, c| { let w = c.with(2)?; let o = w.p_bool(1)?; let (l, e) = super::fnweap::link_end(g, c.sid)?; g.link_end_overlap_event(l, e, o); VOID }),
    // VisualDeletableDetachedToolBehaviour.OnUserDelete: Object.Destroy(gameObject), the end with it.
    ("VisualDeletableDetachedToolBehaviour.OnUserDelete", |g, c| { c.with(1)?; super::fnweap::link_end(g, c.sid)?; g.tr_destroy_component(c.sid, 0.0); VOID }),
    ("LinkDeviceBehaviour.Awake", |g, c| { let w = c.with(1)?; w.p_usize(g, 0)?; VOID }),
    ("LinkDeviceBehaviour.OnUserDelete", |g, c| { let w = c.with(1)?; w.p_usize(g, 0)?; VOID }),
    ("LinkDeviceBehaviour.CanBeDeleted", |g, c| { let w = c.with(1)?; w.p_usize(g, 0)?; Ok(bit(g.ShowLimbStatus)) }),
    ("LinkDeviceBehaviour.Update", |g, c| { let w = c.with(1)?; let l = w.p_usize(g, 0)?; g.phase_link_update(l); VOID }),
    ("LinkDeviceBehaviour.OnMouseOverlapEvent", |g, c| { let w = c.with(2)?; let l = w.p_usize(g, 0)?; let o = w.p_bool(1)?; g.link_device_overlap_event(l, o); VOID }),
    ("LinkDeviceBehaviour.IsMouseInsideCollider", |g, c| { let w = c.with(1)?; let l = w.p_usize(g, 0)?; Ok(bit(g.link_device_inside_collider(l))) }),
    ("LinkDeviceBehaviour.GetVisualBounds", |g, c| { let w = c.with(1)?; let l = w.p_usize(g, 0)?; let (cn, ex) = g.link_device_bounds(l); Ok(format!("Bounds(m_Center={} m_Extents={})", vec(cn), vec(ex))) }),
    // fnweap: WoodStrutBehaviour.Start run again (DistanceJointWireBehaviour.Start, breakForceBuffer, the legacy colour test).
    // fnweap: EnergyWireBehaviour's antenna and egg pairs (fnw20, energy wires between spawned items).
    ("EnergyWireBehaviour.SetAntennaObjects", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.energy_wire_set_antenna_objects(w); VOID }),
    ("EnergyWireBehaviour.SetEggs", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.energy_wire_set_eggs(w); VOID }),
    ("EnergyWireBehaviour.TransferAntennaSignal", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.energy_wire_transfer_antenna_signal(w); VOID }),
    ("WoodStrutBehaviour.Start", |g, c| { let w = c.with(1)?; let x = w.p_usize(g, 0)?; g.blood_wires[x].started = false; g.blood_wire_start(x); VOID }),
    // fnweap: SliderJointWireBehaviour.Start run again (the typedJoint branch, autoConfigureAngle, useLimits, Initialise).
    ("SliderJointWireBehaviour.Start", |g, c| { let w = c.with(1)?; let x = w.p_usize(g, 0)?; g.blood_wires[x].started = false; g.blood_wire_start(x); VOID }),
    // fnweap: the whole Start (DistanceJointWireBehaviour.Start, Initialise, then NewUseWireBehaviour's part).
    ("NewUseWireBehaviour.Start", |g, c| { let w = c.with(1)?; let x = w.p_usize(g, 0)?; g.blood_wires[x].started = false; g.blood_wire_start(x); VOID }),
    // fnweap: base.Update (WireBehaviour.Update), then the three vertices.
    ("SliderJointWireBehaviour.Update", |g, c| { let w = c.with(1)?; let x = w.p_usize(g, 0)?; g.blood_wire_update(x); VOID }),
    // fnweap: WireBehaviour.Update's own body (the recorder calls it non-virtually on every wire); a wire before its Start has
    // no lineChild: IsMouseInsideBounds = false.
    ("WireBehaviour.Update", |g, c| { let w = c.with(1)?; let x = w.p_usize(g, 0)?; if g.paused_menu { return VOID; } if g.blood_wires[x].started { g.wire_base_update(x) } else { g.blood_wires[x].IsMouseInsideBounds = false } VOID }),
    // fnweap: CopperWireBehaviour. Tick: EnergyWireBehaviour.Tick, then the burn; SetWireColours: empty; Start run again.
    ("CopperWireBehaviour.Tick", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.energy_wire_tick(w); g.wire_burn(w); VOID }),
    ("CopperWireBehaviour.Slice", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.wire_slice(w); VOID }),
    // batch4 r6: the spring cable's initialBreakForce (the rewrite's breakForceBuffer) and the strut's breakForceBuffer = 0.
    ("SpringCableBehaviour.Slice", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.wire_slice(w); VOID }),
    // weapons r22: SpringCableBehaviour. Update: WireBehaviour.Update, then the line (blood_wire_update's spring arm); a wire
    // before Start has no lineChild (base: IsMouseInsideBounds = false). Tick: the burn. GetVertexCount: vertexCount - 1 (0
    // before Start: -1). JointBroken: the spring_snap clip (a Random draw) while physicalBehaviour lives.
    ("SpringCableBehaviour.Update", |g, c| { let c = c.with(1)?; let x = c.p_usize(g, 0)?; if g.paused_menu { return VOID; } if g.blood_wires[x].started { g.blood_wire_update(x) } else { g.blood_wires[x].IsMouseInsideBounds = false } VOID }),
    ("SpringCableBehaviour.Tick", |g, c| { let c = c.with(1)?; let x = c.p_usize(g, 0)?; g.spring_cable_tick(x); VOID }),
    ("SpringCableBehaviour.GetVertexCount", |g, c| { let c = c.with(1)?; let x = c.p_usize(g, 0)?; let w = &g.blood_wires[x]; Ok((if w.started { w.vertex_count as isize } else { -1 }).to_string()) }),
    ("SpringCableBehaviour.JointBroken", |g, c| { let c = c.with(1)?; let x = c.p_usize(g, 0)?; g.wire_joint_broken(x); VOID }),
    ("WoodStrutBehaviour.Slice", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.wire_slice(w); VOID }),
    // batch4 r9: TeslaCoil OnImmobilityCapture/Release(sender): whCapturers loaded from the PI record (the gen pairing passed the
    // coil's own id as the collider: a no-op); the message reaches the coil through one of its colliders, as SendMessage does.
    ("TeslaCoilBehaviour.OnImmobilityCapture", |g, c| { let (i, f, col) = tesla_case(g, c)?; let _ = i; g.tesla_coil_on_immobility_capture(col, f); VOID }),
    ("TeslaCoilBehaviour.OnImmobilityRelease", |g, c| { let (i, f, col) = tesla_case(g, c)?; let _ = i; g.tesla_coil_on_immobility_release(col, f); VOID }),
    // batch4 r9: OnDebrisCreated on FnTiny's DebrisPrefab clone (arg 0: a Spawned root of the PI, Awake only, as Break's).
    ("VaseBehaviour.OnDebrisCreated", |g, c| {
        let w = c.with_self(3)?;
        let x = g.destroyables.iter().position(|d| d.id == w.sid()).ok_or("no VaseBehaviour")?;
        let debris = reference(w.arg(1)?).ok_or("null debris")?;
        let v = w.p_arr::<2>(2)?;
        g.vase_on_debris_created_loaded(x, debris, v)?;
        VOID
    }),
    // batch4 r7: the game's Tick takes no argument (the gen pairing passed the stress threshold).
    ("WoodStrutBehaviour.Tick", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.wood_strut_tick(w); VOID }),
    // gapC r15: no-case functions with a rewrite twin but no entry (fngc17a_gen).
    ("HeatPipeBehaviour.Tick", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.heat_pipe_tick(w); VOID }),
    ("MagnetBehaviour.Use", |g, c| { let c = c.with(1)?; let m = c.p_usize(g, 0)?; g.magnet_behaviour_use(m, super::super::translated::rt::Activation { Channel: 0 }); VOID }),
    // gapC r18: functions with recorded cases and a rewrite function but no entry (sized across every recording).
    // Stab(stabber wound normal point victim): the victim is the self limb's physical; MapConfigProbe's wound=0 turned the
    // stabber's StabCausesWound off for the call and ListenPost turns it back on before the PO snapshot (as CirculationBehaviour.Stabbed).
    ("LimbBehaviour.Stabbed", |g, c| { let l = c.comp("LimbBehaviour")?; let s = stab_arg(g, c, l)?; g.limb_stabbed(l, s); stab_probe_reset(g, c)?; VOID }),
    ("CirculationBehaviour.Unstabbed", |g, c| { let l = c.comp("CirculationBehaviour")?; let s = stab_arg(g, c, l)?; g.circulation_unstabbed(l, s); stab_probe_reset(g, c)?; VOID }),
    // RequestFreeze(Rigidbody2D a): the PhysicalBehaviour whose rigidbody it is.
    ("FreezeStackController.RequestFreeze", |g, c| { let c = c.with(1)?; let a0 = c.p_usize(g, 0)?; g.freeze_request(a0); VOID }),
    // IgnoreCollisionSubstituteMethod(Collider2D a, Collider2D b, bool ignore): the colliders' instance ids.
    ("IgnoreCollisionStackController.IgnoreCollisionSubstituteMethod", |g, c| { let c = c.with(3)?; let (a, b) = (c.p_int(0)?, c.p_int(1)?); let i = c.p_bool(2)?; g.ignore_collision_substitute(a, b, i); VOID }),
    // gapC r20: Tritium.OnEnterContainer/OnExitContainer: the container's GlowInTheDarkBehaviour before the call (the PI Clone `comps=`;
    // a seed's AddComponent or an earlier case's OnEnterContainer) is ours' glow under the game's id; its Start ran or not, the calls
    // read only its presence.
    ("Tritium.OnEnterContainer", |g, c| { let w = c.with(2)?; let (a0, a1) = (w.p_usize(g, 0)?, w.p_owner(g, 1)?); glow_pre(g, c, a1); g.tritium_on_enter_container(a0, a1); VOID }),
    ("Tritium.OnExitContainer", |g, c| { let w = c.with(2)?; let (a0, a1) = (w.p_usize(g, 0)?, w.p_owner(g, 1)?); glow_pre(g, c, a1); g.tritium_on_exit_container(a0, a1); VOID }),
    // gapC r21: the container's ExplosiveBehaviour before the call (a seed's Nitroglycerine.OnEnterContainer added it: nitro_pre).
    ("Nitroglycerine.OnExitContainer", |g, c| { let w = c.with(2)?; let (a0, a1) = (w.p_usize(g, 0)?, w.p_owner(g, 1)?); nitro_pre(g, c, a1); g.nitroglycerine_on_exit_container(a0, a1); VOID }),
    // gapC r21: the PI record's instantiatedWhO (a seed's WhOpening clone, `@..#<GameObject id>`; null: none) is ours'.
    ("TeslaCoilBehaviour.OnDestroy", |g, c| {
        c.with(0)?;
        let i = g.tesla_coils.iter().position(|x| x.id == c.sid).ok_or("no TeslaCoilBehaviour")?;
        let r = c.s.of("ItemScript").find(|r| r.id == c.sid).ok_or("no TeslaCoil record")?;
        g.tesla_coils[i].instantiatedWhO = r.get("instantiatedWhO").and_then(reference);
        g.tesla_coil_on_destroy(i);
        VOID
    }),
    // gapC r21: Object.Destroy(base.gameObject) (the Listener::DestroyThis call).
    ("DetachFromParentOnStart.DestroyThis", |g, c| { c.with(0)?; g.destroy_object(c.sid); VOID }),
    // gapC r23: Destroy(FirearmAttachment) of a connected point (FnTiny's even cases) and Destroy(audioProvider) (the PI's FuseBox clip=).
    ("FirearmAttachmentPointBehaviour.OnDestroy", |g, c| { c.with(0)?; let i = g.attachment_points.iter().position(|x| x.id == c.sid).ok_or("no attachment_points entry")?; g.attachment_point_on_destroy(i); VOID }),
    ("FuseBoxBehaviour.OnDestroy", |g, c| { c.with(0)?; g.fuse_box_on_destroy(c.sid); VOID }),
    ("BulbBehaviour.OnDestroy", |g, c| { c.with(0)?; let i = g.bulbs.iter().position(|x| x.id == c.sid).ok_or("no bulbs entry")?; g.bulb_on_destroy(i); VOID }),
    // Defibrillate(PhysicalBehaviour pad): the self defibrillator's PadLayer.
    ("DefibrillatorBehaviour.Defibrillate", |g, c| { let w = c.with_self(2)?; let d = g.defibrillators.iter().position(|x| x.id == c.sid).ok_or("no defibrillators entry")?; let pad = w.p_usize(g, 1)?; let layer = g.defibrillators[d].PadLayer; g.defibrillate(d, pad, layer); VOID }),
    // AffectCollider(Collider2D collider, Vector2 dir, Vector2 point): the collider's PhysicalBehaviour (PhysicalObjectsInWorldByTransform).
    ("FlamethrowerBehaviour.AffectCollider", |g, c| {
        let w = c.with_self(4)?;
        let i = g.flamethrowers.iter().position(|x| x.id == c.sid).ok_or("no flamethrowers entry")?;
        let collider: i32 = w.p_int(1)?;
        let physical = g.physical_of_collider(collider);
        g.flamethrower_affect_collider(i, physical, collider, w.p_arr::<2>(2)?, w.p_arr::<2>(3)?);
        VOID
    }),
    // GetLocalJointAnchor(): FirearmAttachment and firearmBehaviour as the point holds them.
    ("FirearmAttachmentPointBehaviour.GetLocalJointAnchor", |g, c| {
        c.with_self(1)?;
        let pt = g.attachment_points.iter().position(|x| x.id == c.sid).ok_or("no attachment_points entry")?;
        let nre = || super::super::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        let a = g.attachment_points[pt].FirearmAttachment.unwrap_or_else(nre);
        let gun = g.attachment_points[pt].firearm.and_then(|f| g.firearms[f].physical).unwrap_or_else(nre);
        Ok(Enc::enc(g.attachment_local_joint_anchor(pt, a, gun)))
    }),
    // CalculateVisualiser(): the jukebox's own AudioSource.
    ("JukeboxBehaviour.CalculateVisualiser", |g, c| { c.with_self(1)?; let j = g.jukeboxes.iter().position(|x| x.id == c.sid).ok_or("no jukeboxes entry")?; let s = g.jukebox_source(j); g.jukebox_calculate_visualiser(j, s); VOID }),
    // Liquid.OnUpdate(container): the case's liquid= and the container.
    ("Liquid.OnUpdate", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_owner(g, 1)?; g.liquid_on_update(a0, a1); VOID }),
    // SteamManagerCheck(): StatManager.attempts as the case's sfpre= gives it.
    ("StatManager.SteamManagerCheck", |_g, c| {
        c.with(0)?;
        let a = c.field("sfpre").and_then(|v| v.split(';').find_map(|x| x.strip_prefix("StatManager.attempts:"))).ok_or("no sfpre StatManager.attempts")?;
        super::super::stats::check_seed(a.parse().map_err(|_| format!("attempts {a}"))?);
        Ok(Enc::enc(super::super::stats::steam_manager_check()))
    }),
    // gapB r20: SentryTurretBehaviour.PlaySound(set): GenRun's AudioClip[] (`[@name#id;..;null]`), each clip by its name.
    ("SentryTurretBehaviour.PlaySound", |g, c| {
        c.with(1)?;
        let i = g.sentry_turrets.iter().position(|x| x.id == c.sid).ok_or("no SentryTurretBehaviour")?;
        let a = c.args[0].strip_prefix('[').and_then(|x| x.strip_suffix(']')).ok_or("PlaySound arg not an array")?;
        let set: Vec<_> = a.split(';').map(|x| x.strip_prefix('@').and_then(|x| x.rsplit_once('#')).map(|(n, _)| std::sync::Arc::new(super::super::audio::Clip {
            asset: Default::default(),
            clip: crate::assets::classes::AudioClip { name: n.to_string(), load_type: 0, channels: 1, frequency: 44100, length: 1.0, resource: Default::default(), compression_format: 0 },
            pcm: None,
            stream: None,
        }))).collect();
        g.sentry_play_clips(i, &set);
        VOID
    }),
    // gapB r20: OnJointBreak2D(broken) on the grenade's own GameObject: DestroyJoint when broken is its joint (the case's
    // Joint2D arg as its `fixed_joints` index; ITEM_CALLS' entry passed the grenade's index for it).
    // gapB r21: spawn-time Awakes again, with the instance's link-time context (Game::awake_ctx).
    ("IndustrialLiftBehaviour.Awake", |g, c| { c.with(0)?; let (x, o, _) = g.awake_ctx.get(&c.sid).cloned().ok_or("no link-time context")?; g.industrial_lift_awake_at(x.2.industrial_lifts[&o], &x.0, &x.1, &x.2); VOID }),
    ("ReactorConduitBehaviour.Awake", |g, c| { c.with(0)?; let (x, o, _) = g.awake_ctx.get(&c.sid).cloned().ok_or("no link-time context")?; g.reactor_conduit_awake_at(x.2.reactor_conduits[&o], &x.0, &x.1, &x.2, o); VOID }),
    ("TeslaCoilBehaviour.Awake", |g, c| { c.with(0)?; let (x, o, _) = g.awake_ctx.get(&c.sid).cloned().ok_or("no link-time context")?; g.unity_callback(|g| g.tesla_coil_awake(x.2.tesla_coils[&o], &x.2.tesla_bolt_sources[&o])); VOID }),
    // gapB r21 (index audit): WireBehaviour.OnJointBreak2D's Joint2D arg is the joint's adopted `fixed_joints` entry (fncheck_gen's
    // p_usize gave its index among its record type's records); the parameterless JointBroken overrides take the self wire (fncheck_gen
    // passed the wire's index where blood_wire_on_joint_break wants a joint's).
    ("WireBehaviour.OnJointBreak2D", |g, c| {
        c.with(1)?;
        // A wire's own typedJoint (load_wires' entry, its `joint`), else any other adopted joint (no wire holds it).
        let id = reference(c.args[0]);
        let own = id.and_then(|id| super::fnweap::wire_records(c.s).into_iter().find(|r| r.get("typedJoint").and_then(reference) == Some(id))).and_then(|r| super::fnweap::wire_index(g, r.id)).map(|w| g.blood_wires[w].joint);
        if let Some(f) = own.or_else(|| c.adopted_joint(g, 0)) { g.blood_wire_on_joint_break(f); }
        VOID
    }),
    ("WoodStrutBehaviour.JointBroken", |g, c| { let w = c.with(1)?.p_usize(g, 0)?; g.wire_joint_broken(w); VOID }),
    ("WoodenBindingBehaviour.JointBroken", |g, c| { let w = c.with(1)?.p_usize(g, 0)?; g.wire_joint_broken(w); VOID }),
    ("StickyGrenadeBehaviour.OnJointBreak2D", |g, c| {
        c.with(1)?;
        let i = g.sticky_grenades.iter().position(|x| x.id == c.sid).ok_or("no sticky_grenades entry")?;
        let _ = i;
        if let Some(f) = c.adopted_joint(g, 0) { g.sticky_grenade_on_joint_break(f); }
        VOID
    }),
    // gapB r20: Explode(ex) (GenRun's ExplosionParameters) or Explode(center, force).
    ("ExplosionCreator.Explode", |g, c| {
        if let Some(a) = c.args.first().filter(|a| a.starts_with("ExplosionParameters(")) { c.with(1)?; let ex = explosion_arg(a)?; g.explosion_creator_explode(ex); }
        else { let w = c.with(2)?; g.explosion_creator_explode_at(w.p_arr::<3>(0)?, w.p_f32(1)?); }
        VOID
    }),
    // The ExplosionCreator statics (ExplosionParameters as Dump.Format writes it).
    // v52h r12: GlowInTheDarkBehaviour.OnDestroy called on a live component (the probe's call): its body only.
    ("GlowInTheDarkBehaviour.OnDestroy", |g, c| { c.with(0)?; if let Some(i) = g.glows.iter().position(|x| x.id == c.sid) { g.glow_on_destroy(i); } VOID }),
    // (none: its glow and lightSprite destroyed or never made (`glow=destroyed`), both ifs false.)
    // v52h r12: ResizeHandles.Awake: camera = Camera.main, which the rewrite keeps as the Game's camera (no state of its own).
    ("ResizeHandles.Awake", |_, c| { c.with(0)?; VOID }),
    // v52h r12: and the [Obsolete] overload (rayCount, position, range, fragmentForce, particleAndSound, big, dismemberChance): its
    // ExplosionParameters initialiser (LargeExplosionParticles = big: EffectSize Large or Medium; BallisticShrapnelCount 0).
    ("ExplosionCreator.CreateFragmentationExplosion", |g, c| {
        let ex = if c.args.len() == 7 {
            let w = c.with(7)?;
            super::super::explosion::ExplosionParameters::new_big(w.p_int(0)?, w.p_arr::<3>(1)?, w.p_f32(2)?, w.p_f32(3)?, w.p_bool(4)?, w.p_bool(5)?, w.p_f32(6)?, 0)
        } else {
            c.with(1)?;
            explosion_arg(c.args[0])?
        };
        g.create_fragmentation_explosion(ex);
        VOID
    }),
    // CreateExplosionWithWater(isUnderWater, waterSurfaceLevel, ex, doSplash) (the obsolete overload: the level is unused) or (isUnderWater, ex, doSplash).
    ("ExplosionCreator.CreateExplosionWithWater", |g, c| {
        let ex = explosion_arg(c.args.iter().find(|a| a.starts_with("ExplosionParameters(")).ok_or("no ExplosionParameters arg")?)?;
        let under = c.args.first() == Some(&"1");
        let splash = c.args.last().is_none_or(|a| *a != "0");
        g.create_explosion_with_water_splash(under, ex, splash);
        VOID
    }),
    // batch4 r7: InputSystem.Held("fast") as the input state holds it (the game's own read inside StopSelectionDrag).
    ("ToolControllerBehaviour.StopSelectionDrag", |g, c| { c.with(0)?; let fast = g.input.held(crate::il!(str "ToolControllerBehaviour::StopSelectionDrag" 0)); g.stop_selection_drag(fast); VOID }),
    // batch4 r8: HandleToolSelectionInput with FnTiny's one tool key Down (FnCase ` toolkey=`) and the recorded cycle fields
    // (parentCycleIndex, childCycleIndex, childCount); fn_check compares them and the chosen tool after.
    ("ToolControllerBehaviour.HandleToolSelectionInput", |g, c| {
        c.with(0)?;
        let r = c.s.of("ToolControllerBehaviour").next().ok_or("no ToolControllerBehaviour record")?;
        let int = |k: &str| -> Result<i32, String> { r.req(k)?.parse().map_err(|_| format!("bad {k}")) };
        let (p, ch, n) = (int("parentCycleIndex")?, int("childCycleIndex")?, int("childCount")?);
        let t = g.tool.as_mut().ok_or("no tool")?;
        (t.parentCycleIndex, t.childCycleIndex, t.childCount) = (p, ch, n);
        if let Some(key) = c.s.of("FnCase").next().and_then(|r| r.get("toolkey")) {
            g.input.controls.insert(key.to_string(), crate::game::tool::Control { down: true, held: false, up: false });
        }
        g.handle_tool_selection_input();
        VOID
    }),
    // batch4 r8: SpringWireTool.OnJointCreate(joint) on the recorder's MakeArg SpringJoint2D (fnweap::load_wires adopts it): the
    // shared wire-tool body with the SpringWire kind (use_wire_tool_on_joint_create swaps the kind in and back).
    // batch4 r12: the base body, on the SpringWireTool (FnTiny chooses it; FnWeap.NonVirtual calls it).
    ("SpringJointWireTool.OnJointCreate", |g, c| {
        c.with(1)?;
        let f = c.adopted_joint(g, 0).ok_or("joint arg: not adopted")?;
        g.spring_joint_wire_tool_on_joint_create(f);
        VOID
    }),
    ("SpringWireTool.OnJointCreate", |g, c| {
        c.with(1)?;
        let f = c.adopted_joint(g, 0).ok_or("joint arg: not adopted")?;
        let kind = super::super::blood_wire::WireKind::of_tool("SpringWireTool").ok_or("no SpringWireTool kind")?;
        g.use_wire_tool_on_joint_create(f, kind);
        VOID
    }),
    // batch1 r7: ConditionalTextBehaviour.Update of one of the seven (FnTiny's " ctpre=": t, text, foundProperty set, Property,
    // Time.unscaledDeltaTime; " ctsel=": the selection), its t and text after the call returned as the recorder's " ctt=".
    // batch1 r8: AntennaReceiverBehaviour.GetNextFloat(index, count) (FnTiny's " arpre=": SignalStrength, OldFactor, CurrentFactor,
    // currentAudioPosition, the rand's inext, inextp and SeedArray): the float and the rand after the call (" rnd=").
    ("AntennaReceiverBehaviour.GetNextFloat", |g, c| {
        let _ = g;
        let pre = c.field("arpre").ok_or("no arpre= (a recording before batch1 r8)")?;
        let v: Vec<&str> = pre.split(',').collect();
        if v.len() != 7 {
            return Err(format!("arpre= has {} values", v.len()));
        }
        let hx = |s: &str| u32::from_str_radix(s, 16).map(f32::from_bits).map_err(|_| format!("arpre {s}"));
        let n = |s: &str| s.parse::<i64>().map_err(|_| format!("arpre {s}"));
        let seeds: Vec<i32> = v[6].split(':').map(|x| x.parse().map_err(|_| format!("SeedArray {x}"))).collect::<Result<_, String>>()?;
        let seeds: [i32; 56] = seeds.try_into().map_err(|_| "SeedArray is not 56 ints")?;
        let mut rand = crate::game::utils::NetRandom::from_state(seeds, n(v[4])? as usize, n(v[5])? as usize);
        let r = crate::game::antenna::Radio { SignalStrength: hx(v[0])?, OldFactor: hx(v[1])?, CurrentFactor: hx(v[2])?, currentAudioPosition: n(v[3])? as i32 };
        let (index, count): (i32, i32) = (c.arg(0)?, c.arg(1)?);
        let x = crate::game::antenna::get_next_float(&r, &mut rand, index, count);
        Ok(format!("{:08x} rnd={}", x.to_bits(), rand.state_string()))
    }),
    // batch1 r10: BurnableSpriteBehaviour.Awake: the renderer's block as FnTiny left it (" bspre=": none = cleared, else its _Progress
    // hex; no record holds the block); the component's new propertyBlock after, as the recorder's " bsa=<isEmpty>:<_Progress>".
    ("BurnableSpriteBehaviour.Awake", |g, c| {
        c.with(0)?;
        let b = g.burnable_sprites.iter().position(|x| x.id == c.sid).ok_or("no burnable_sprites entry")?;
        let at = crate::game::visual::SpriteAt::Physical(g.burnable_sprites[b].physical);
        let mut block = crate::game::props::PropBlock::default();
        match c.field("bspre").ok_or("no bspre= (a recording before batch1 r10)")? {
            "none" => {}
            h => block.set_float("_Progress", f32::from_bits(u32::from_str_radix(h, 16).map_err(|_| format!("bspre {h}"))?)),
        }
        g.set_property_block(at, &block);
        g.burnable_sprites[b].block.set_float("_Progress", 0.5);
        g.burnable_sprite_awake(b);
        let own = &g.burnable_sprites[b].block;
        Ok(format!(" bsa={}:{}", own.0.is_empty() as i32, hex(own.get_float("_Progress"))))
    }),
    // batch1 r10: ChainsawBehaviour.OnBeforeSerialise: the PI record's softConnections (FnTiny seeds them with the private SoftConnect),
    // each joint a FrictionJoint2D on its victim to the saw (destroyed when the record says null), and the limbs' UniqueIdentity
    // (" csid="); SerialisableVictims after as the recorder's " sv=" (Guid "N" hex).
    ("ChainsawBehaviour.OnBeforeSerialise", |g, c| {
        let i = chainsaw_with_connections(g, c)?;
        g.chainsaw_on_before_serialise(i);
        let sv: Vec<String> = g.chainsaws[i].SerialisableVictims.iter().map(|x| format!("{x:032x}")).collect();
        Ok(format!(" sv={}", sv.join(",")))
    }),
    // batch1 r11: ContraptionOutlineSerialiser.LoadOutline(path): FnTiny's file bytes (" olt=" hex) written to a temp file, read back.
    ("ContraptionOutlineSerialiser.LoadOutline", |_g, c| {
        let h = c.field("olt").ok_or("no olt=")?;
        let bytes: Vec<u8> = (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16)).collect::<Result<_, _>>().map_err(|_| "olt hex")?;
        let path = std::env::temp_dir().join(format!("ppg-fncheck-outline-{}-{}.txt", std::process::id(), c.id));
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        let r = crate::game::contraption::load_outline(&path);
        let _ = std::fs::remove_file(&path);
        Ok(outline_fmt(&r?))
    }),
    // batch1 r11: ContraptionOutlineSerialiser.GenerateOutline(physicalBehaviours, center): the arg limbs' roots from the PI (" csid=" identities).
    ("ContraptionOutlineSerialiser.GenerateOutline", |g, c| {
        csid_identities(g, c)?;
        let w = c.with(2)?;
        // A null entry: `!physicalBehaviour`, skipped.
        let sel: Vec<usize> = w.p_list(0, |a| reference(a).map(|id| phys_by_id(g, id)).transpose())?.into_iter().flatten().collect();
        let center = w.p_arr::<3>(1)?;
        let roots = g.contraption_outline_roots(&sel);
        Ok(outline_fmt(&crate::game::contraption::generate_outline(&roots, center)))
    }),
    // batch1 r12: ContraptionOutlineSerialiser.SaveOutline(metadata, gameObjects, center): the gameObjects' PhysicalBehaviours
    // (" olgo=", 0 for none: the `where a` filter), GenerateOutline, then the text to a temp PathToOutlineFile; its bytes as " olw=".
    ("ContraptionOutlineSerialiser.SaveOutline", |g, c| {
        csid_identities(g, c)?;
        let mut sel = Vec::new();
        for x in c.field("olgo").ok_or("no olgo=")?.split(',') {
            match x.parse::<i32>().map_err(|_| format!("olgo {x}"))? {
                0 => {}
                id => sel.push(phys_by_id(g, id)?),
            }
        }
        let center = c.with(3)?.p_arr::<3>(2)?;
        let roots = g.contraption_outline_roots(&sel);
        let rects = crate::game::contraption::generate_outline(&roots, center);
        let path = std::env::temp_dir().join(format!("ppg-fncheck-saveoutline-{}-{}.outline", std::process::id(), c.id));
        let meta = crate::game::contraption::ContraptionMetaData { PathToOutlineFile: Some(path.clone()), ..Default::default() };
        crate::game::contraption::save_outline(&meta, &rects)?;
        let bytes = std::fs::read(&path).map_err(|e| e.to_string());
        let _ = std::fs::remove_file(&path);
        Ok(format!(" olw={}", bytes?.iter().map(|b| format!("{b:02x}")).collect::<String>()))
    }),
    // fc41 r4: the saws' ProcessSoftConnections(shouldSaw, strength) / OnAfterDeserialise (SharpOnAllSides also FixedUpdate,
    // OnDestroy) on FnTiny.R4Seed's connections (PI softConnections, the seeded maxForce), then " st=sc:" (FnTiny.R4Of).
    ("ChainsawBehaviour.ProcessSoftConnections", |g, c| {
        let i = g.chainsaws.iter().position(|x| x.id == c.sid).ok_or("no chainsaws entry")?;
        let saw = g.chainsaws[i].physical;
        g.chainsaws[i].softConnections = pi_soft_connections(g, c, saw)?;
        g.chainsaw_process(i, c.arg(0)?, c.arg(1)?);
        let conns = g.chainsaws[i].softConnections.clone();
        Ok(soft_state(g, &conns))
    }),
    // fc41 r5: OnDestroy on the seeded soft connections (Destroy calls of each joint compared).
    ("ChainsawBehaviour.OnDestroy", |g, c| { let i = chainsaw_with_connections(g, c)?; g.chainsaw_destroy(i); let conns = g.chainsaws[i].softConnections.clone(); Ok(soft_state(g, &conns)) }),
    ("EnergySwordBehaviour.OnDestroy", |g, c| {
        let i = g.energy_swords.iter().position(|x| x.id == c.sid).ok_or("no energy_swords entry")?;
        let saw = g.energy_swords[i].physical;
        g.energy_swords[i].softConnections = pi_soft_connections(g, c, saw)?;
        let blur = g.with_component_instance(c.sid, |g, inst, _| inst.objects[g.energy_swords[i].MotionBlur].components[0]);
        g.energy_sword_destroy(i, blur);
        let conns = g.energy_swords[i].softConnections.clone();
        Ok(soft_state(g, &conns))
    }),
    ("ChainsawBehaviour.OnAfterDeserialise", |g, c| {
        let i = g.chainsaws.iter().position(|x| x.id == c.sid).ok_or("no chainsaws entry")?;
        let saw = g.chainsaws[i].physical;
        g.chainsaws[i].softConnections = pi_soft_connections(g, c, saw)?;
        let (sv, objects) = r4_victims(g, c)?;
        g.chainsaws[i].SerialisableVictims = sv;
        g.chainsaw_on_after_deserialise(i, &objects);
        let conns = g.chainsaws[i].softConnections.clone();
        Ok(soft_state(g, &conns))
    }),
    ("EnergySwordBehaviour.ProcessSoftConnections", |g, c| {
        let i = g.energy_swords.iter().position(|x| x.id == c.sid).ok_or("no energy_swords entry")?;
        let saw = g.energy_swords[i].physical;
        g.energy_swords[i].softConnections = pi_soft_connections(g, c, saw)?;
        g.energy_sword_process(i, c.arg(0)?, c.arg(1)?);
        let conns = g.energy_swords[i].softConnections.clone();
        Ok(soft_state(g, &conns))
    }),
    ("EnergySwordBehaviour.OnAfterDeserialise", |g, c| {
        let i = g.energy_swords.iter().position(|x| x.id == c.sid).ok_or("no energy_swords entry")?;
        let saw = g.energy_swords[i].physical;
        g.energy_swords[i].softConnections = pi_soft_connections(g, c, saw)?;
        let (sv, objects) = r4_victims(g, c)?;
        g.energy_swords[i].SerialisableVictims = sv;
        g.energy_sword_on_after_deserialise(i, &objects);
        let conns = g.energy_swords[i].softConnections.clone();
        Ok(soft_state(g, &conns))
    }),
    ("SharpOnAllSidesBehaviour.ProcessSoftConnections", |g, c| { let s = sharp_with_connections(g, c)?; g.sharp_on_all_sides_process(s, c.arg(0)?, c.arg(1)?); Ok(sharp_state(g, s)) }),
    ("SharpOnAllSidesBehaviour.FixedUpdate", |g, c| { let s = sharp_with_connections(g, c)?; g.sharp_on_all_sides_fixed_update(s); Ok(sharp_state(g, s)) }),
    ("SharpOnAllSidesBehaviour.OnDestroy", |g, c| { let s = sharp_with_connections(g, c)?; g.sharp_on_all_sides_destroy(s); Ok(sharp_state(g, s)) }),
    // fc41 r13: OnDisable is OnDestroy (the same seeded connections, FnTiny's R4).
    ("SharpOnAllSidesBehaviour.OnDisable", |g, c| { let s = sharp_with_connections(g, c)?; g.sharp_on_all_sides_on_disable(s); Ok(sharp_state(g, s)) }),
    ("SharpOnAllSidesBehaviour.OnAfterDeserialise", |g, c| {
        let s = sharp_with_connections(g, c)?;
        let (sv, objects) = r4_victims(g, c)?;
        g.sharp_on_all_sides[s].SerialisableVictims = sv;
        // fc41 r6: SoftConnect's Stabbed plays the victim's Properties.StabSound (a recording's properties hold no clips).
        super::all_property_clips(g)?;
        g.sharp_on_all_sides_on_after_deserialise(s, &objects);
        Ok(sharp_state(g, s))
    }),
    // fc41 r4: ImmobilityField FixedUpdate / OnTriggerExit2D(collision) / DestroyAllJoints on FnTiny.R4Seed's captures, " st=imf:".
    ("ImmobilityFieldBehaviour.FixedUpdate", |g, c| immobility_entry(g, c, |g, i| g.immobility_field_fixed_update(i))),
    ("ImmobilityFieldBehaviour.DestroyAllJoints", |g, c| immobility_entry(g, c, |g, i| g.immobility_field_destroy_all_joints(i))),
    ("ImmobilityFieldBehaviour.OnTriggerExit2D", |g, c| { let coll: i32 = c.arg(0)?; immobility_entry(g, c, |g, i| g.immobility_field_trigger_exit(i, coll)) }),
    // fc41 r4: TyreDeflation.Awake from FnTiny's wrong `initial` (" stpre=ty:<Radius>,<Offset x>,<y>"), " st=ty:" after.
    ("TyreDeflationBehaviour.Awake", |g, c| {
        let t = arena(&g.tyres, c.sid, |x| x.id, "tyres")?;
        let pre: Vec<f32> = st_pre(c).and_then(|v| v.strip_prefix("ty:")).unwrap_or("").split(',').filter_map(|x| u32::from_str_radix(x, 16).ok()).map(f32::from_bits).collect();
        if let [r, x, y] = pre[..] { g.tyres[t].initialRadius = r; g.tyres[t].initialOffset = [x, y]; }
        g.with_component_instance(c.sid, |g, inst, _| g.tyre_awake(t, inst));
        let (r, o) = (g.tyres[t].initialRadius, g.tyres[t].initialOffset);
        Ok(format!(" st=ty:{},{},{}", hex(r), hex(o[0]), hex(o[1])))
    }),
    // fc41 r4: TimedGate.Awake from FnTiny's seeded delay (" stpre=tg:"), " st=tg:" after.
    ("TimedGateBehaviour.Awake", |g, c| {
        let i = g.timed_gates.iter().position(|x| x.id == c.sid).ok_or("no timed_gates entry")?;
        let p = g.timed_gates[i].physical;
        if let Some(v) = st_pre(c).and_then(|v| v.strip_prefix("tg:")).and_then(|v| u32::from_str_radix(v, 16).ok()) {
            g.physicals[p].ActivationPropagationDelay = f32::from_bits(v);
        }
        g.timed_gate_awake(i);
        Ok(format!(" st=tg:{}", hex(g.physicals[p].ActivationPropagationDelay)))
    }),
    // fc41 r4: UiColorWave.OnDisable (the eye's) from the recorder's Renderer.color (" stpre=ucw:"), " st=ucw:" after.
    ("UiColorWave.OnDisable", |g, c| {
        g.load_eye_alone()?;
        if let Some(v) = st_pre(c).and_then(|v| v.strip_prefix("ucw:")) {
            let w: Vec<f32> = v.split(',').filter_map(|x| u32::from_str_radix(x, 16).ok()).map(f32::from_bits).collect();
            if let [r, gg, b, a] = w[..] { g.eye.color = [r, gg, b, a]; }
        }
        g.eye_animation_on_disable();
        let k = g.eye.color;
        Ok(format!(" st=ucw:{},{},{},{}", hex(k[0]), hex(k[1]), hex(k[2]), hex(k[3])))
    }),
    // v52h r3: UiColorWave.Update (the eye's) from " stpre=ucw:<Renderer.color>,<oV>,<sV>,<Time.unscaledTime>", the colour it sets after.
    ("UiColorWave.Update", |g, c| {
        g.load_eye_alone()?;
        // stpre "ucw:" r,g,b,a, oV, sV, Time.unscaledTime, then Duration, Offset, OffsetVaration, SpeedVaration, Randomisation (hex) and UseSinWave
        // (0/1): the recorder's Drive/Fuzz set the wave's fields on odd cases.
        let pre = st_pre(c).and_then(|v| v.strip_prefix("ucw:")).ok_or("no stpre=ucw: (a recording before v52h r3)")?;
        let v: Vec<&str> = pre.split(',').collect();
        let [r, gg, b, a, ov, sv, t, dur, off, ovv, svv, rnd] = std::array::from_fn(|k| v.get(k).and_then(|x| u32::from_str_radix(x, 16).ok()).map_or(f32::NAN, f32::from_bits));
        let sin = *v.get(12).ok_or("stpre=ucw: no wave fields (a recording before v52h r3b)")? == "1";
        if let Some(w) = g.eye.wave.as_mut() {
            (w.1, w.2, w.3, w.4, w.5, w.6) = (dur, off, sin, ovv, svv, rnd);
        }
        (g.eye.color, g.eye.oV, g.eye.sV, g.eye.enabled) = ([r, gg, b, a], ov, sv, true);
        g.time.unscaledTimeAsDouble = f64::from(t);
        g.eye_animation_update();
        let (k, w) = (g.eye.color, g.eye.wave.as_ref().ok_or("the eye without its wave")?);
        Ok(format!(" st=ucw:{},{},{},{},{},{},{},{},{},{},{},{},{}", hex(k[0]), hex(k[1]), hex(k[2]), hex(k[3]), hex(g.eye.oV), hex(g.eye.sV), hex(t), hex(w.1), hex(w.2), hex(w.4), hex(w.5), hex(w.6), w.3 as u8))
    }),
    // batch1 r10: ChainsawBehaviour.SoftConnect(otherPhys, coll, connectionStrength) on the loaded softConnections.
    ("ChainsawBehaviour.SoftConnect", |g, c| {
        let i = chainsaw_with_connections(g, c)?;
        let other = match c.args.first().copied() { Some(a) => reference(a).map(|id| phys_by_id(g, id)).transpose()?, None => None };
        let coll = c.args.get(1).and_then(|a| reference(a));
        g.chainsaw_soft_connect(i, other, coll, c.f(2));
        VOID
    }),
    // batch1 r10: ActivationPropagation.Branch(ob) on FnTiny's self (" appre=": Direct, Channel, TraversedCount, ActivityIntensity,
    // Identity, ob's id, the Path's ids): the branch as the recorder's "apb:" (its Direct, Channel, TraversedCount, Identity, Path,
    // then self's ActivityIntensity after).
    ("ActivationPropagation.Branch", |g, c| {
        let pre = c.field("appre").ok_or("no appre= (a recording before batch1 r10)")?;
        let v: Vec<&str> = pre.split(',').collect();
        if v.len() != 7 {
            return Err(format!("appre= has {} values", v.len()));
        }
        let n = |s: &str| s.parse::<i32>().map_err(|_| format!("appre {s}"));
        let ids = |s: &str| -> Result<Vec<i32>, String> { s.split(':').filter(|x| !x.is_empty()).map(n).collect() };
        if v[4] != "0" {
            return Err(format!("Identity {}: 1.28.41 never sets it", v[4]));
        }
        let me = g.signal_with(v[0] == "1", n(v[1])? as u16, n(v[2])?, &ids(v[6])?, n(v[3])?);
        let b = g.signal_branch(&me, n(v[5])?);
        let s = g.sig(&b);
        let mut path = g.signal_path(&b);
        path.sort();
        let path: Vec<String> = path.iter().map(|x| x.to_string()).collect();
        Ok(format!("apb:{},{},{},0,{},{}", s.Direct as i32, s.Channel, s.TraversedCount, path.join(":"), g.sig(&me).ActivityIntensity))
    }),
    // batch1 r8: PersonBehaviour.Awake; FnTiny's " alivedict=0": the transform was out of AliveByTransform (AddToDictionary succeeds).
    ("PersonBehaviour.Awake", |g, c| {
        let w = c.with(1)?;
        let p = w.p_usize(g, 0)?;
        if c.field("alivedict") == Some("0") {
            g.people[p].alive_entry = false;
        }
        // RandomisedSize: transform.localScale = the new scale on the snapshot-loaded person's root (returns void).
        if let Some(s) = g.person_awake(p) {
            g.person_set_root_scale(p, s);
        }
        Ok(String::new())
    }),
    // batch1 r9: BeamCannonBehaviour.OnHit(Transform other, Vector3 pos): other's PhysicalBehaviour in the world.
    ("BeamCannonBehaviour.OnHit", |g, c| {
        let w = c.with(3)?;
        let i = w.p_usize(g, 0)?;
        let (world, _) = transform_physical(g, w.arg(1)?)?;
        g.beam_cannon_on_hit(i, world, w.p_arr::<2>(2)?);
        Ok(String::new())
    }),
    // batch1 r9: BeamRepeaterBehaviour.OnHit(Transform other, Vector3 pos): other's PhysicalBehaviour in the world, and the one on other (its LimbBehaviour).
    ("BeamRepeaterBehaviour.OnHit", |g, c| {
        let w = c.with(3)?;
        let i = w.p_usize(g, 0)?;
        let (world, own) = transform_physical(g, w.arg(1)?)?;
        g.beam_repeater_on_hit(i, world, own, w.p_arr::<2>(2)?);
        Ok(String::new())
    }),
    // batch1 r9: BurnableSpriteBehaviour.OnWillRenderObject: the block's _Progress after (FnTiny " bsp=": no snapshot record holds the block).
    ("BurnableSpriteBehaviour.OnWillRenderObject", |g, c| {
        c.with(0)?;
        let b = g.burnable_sprites.iter().position(|x| x.id == c.sid).ok_or("no burnable_sprites entry")?;
        g.burnable_sprite_will_render(b);
        Ok(format!(" bsp={}", hex(g.burnable_sprites[b].block.get_float("_Progress"))))
    }),
    // batch1 r9: ColorWave.OnWillRenderObject: the PI's ItemScript record's fields (fuzzed ones too; the item loader keeps the
    // prefab's) and oV/sV (OnEnable's draws). The colour is the PO's ItemSprite record.
    ("ColorWave.OnWillRenderObject", |g, c| {
        c.with(0)?;
        let v = g.visuals.iter().position(|x| x.id == c.sid).ok_or("no ColorWave visual")?;
        let r = c.s.of("ItemScript").find(|r| r.id == c.sid).ok_or("no ColorWave ItemScript record")?;
        let hx = |k: &str| r.get(k).and_then(|s| u32::from_str_radix(s, 16).ok()).map(f32::from_bits).ok_or(format!("ColorWave {k}"));
        let crate::game::visual::Kind::ColorWave { Duration, Offset, UseSinWave, OffsetVaration, SpeedVaration, Randomisation, oV, sV, .. } = &mut g.visuals[v].kind else { return Err("not a ColorWave visual".into()) };
        (*Duration, *Offset, *OffsetVaration, *SpeedVaration, *Randomisation, *oV, *sV) = (hx("Duration")?, hx("Offset")?, hx("OffsetVaration")?, hx("SpeedVaration")?, hx("Randomisation")?, hx("oV")?, hx("sV")?);
        *UseSinWave = r.get("UseSinWave") == Some("1");
        g.color_wave_will_render(v);
        Ok(String::new())
    }),
    // batch1 r9: the bolts' OnHit(RaycastHit2D) (BigProbeB.MakeArg's ray).
    ("AcceleratorBoltBehaviour.OnHit", |g, c| {
        let w = c.with(2)?;
        let b = w.p_usize(g, 0)?;
        let hit = ray_arg(g, w.arg(1)?)?;
        g.accelerator_bolt_on_hit(b, &hit);
        Ok(String::new())
    }),
    ("ArchelixCasterBoltBehaviour.OnHit", |g, c| {
        let w = c.with(2)?;
        let b = w.p_usize(g, 0)?;
        let hit = ray_arg(g, w.arg(1)?)?;
        g.archelix_bolt_on_hit(b, &hit);
        Ok(String::new())
    }),
    ("ConditionalTextBehaviour.Update", |g, c| {
        c.with(0)?;
        let pre = c.field("ctpre").ok_or("no ctpre= (a recording before batch1 r7)")?;
        let v: Vec<&str> = pre.split(',').collect();
        if v.len() != 5 {
            return Err(format!("ctpre= has {} values", v.len()));
        }
        let hx = |s: &str| u32::from_str_radix(s, 16).map(f32::from_bits).map_err(|_| format!("ctpre {s}"));
        type G = crate::game::Game;
        let i = G::CONDITIONAL_TEXT.iter().position(|x| x.1 == v[3]).ok_or(format!("no ConditionalTextBehaviour for {}", v[3]))?;
        let sel = match c.field("ctsel").ok_or("no ctsel=")? {
            "-" => Vec::new(),
            s => s.split(':').map(|x| phys_by_id(g, x.parse().map_err(|_| format!("ctsel {x}"))?)).collect::<Result<Vec<_>, _>>()?,
        };
        match g.tool.as_mut() {
            Some(t) => t.rawSelection = sel,
            None => g.tool = Some(super::super::tool::Tool { rawSelection: sel, ..Default::default() }),
        }
        let (_, _, yes, no) = G::CONDITIONAL_TEXT[i];
        let label = |s: &str| s.replace(' ', "_");
        g.context_text_t[i] = hx(v[0])?;
        g.context_text[i] = if v[1] == label(yes) { Some(true) } else if v[1] == label(no) { Some(false) } else { None };
        let before = g.context_text[i];
        g.conditional_text_update_one(i, hx(v[4])?, v[2] == "1");
        let text = if g.context_text[i] == before { v[1].to_string() } else { label(if g.context_text[i] == Some(true) { yes } else { no }) };
        Ok(format!(" ctt={:08x},{text},{},{}", g.context_text_t[i].to_bits(), v[2], v[3]))
    }),
    // batch1 r7: BloodWireTool.OnJointCreate(joint) on the recorder's MakeArg DistanceJoint2D (fnweap::load_wires adopts it); the
    // generated entry passed (tool, joint) as (physical, fixed joint).
    // gapC r2: OnToolChosen on a tool the recorder added (no tool record): wire_tool_chosen.
    ("BloodWireTool.OnToolChosen", wire_tool_chosen),
    // gapC r2: the Muzzleflash's rotation after Start (the recorder's " muz="), the effect after Awake (" teff=": its Transform id,
    // hasEffect, isPlaying).
    ("BlasterBehaviour.Start", |g, c| { let i = c.with(1)?.p_usize(g, 0)?; g.blaster_start(i); Ok(format!(" muz={}", g.blaster_muzzle_rotation(i).map_or("null".into(), |q| q.map(hex).join(",")))) }),
    // gapC r3: CreateEffectObject on a Push/Pull/Lift self (AOEPowerTool's runs the override, as the game's virtual call):
    // ret the new clone's GameObject (its Transform's id + 2).
    ("AOEPowerTool.CreateEffectObject", power_create_effect),
    // fnrec r3: FixedUpdate is the self tool's own (GEN ran every tool's: the other tools' mouseMovement changed too).
    ("AOEPowerTool.FixedUpdate", |g, c| {
        c.with(0)?;
        let k = c.self_kind(crate::game::power::PowerKind::of_tool)?;
        let p = g.powers.iter().position(|p| p.kind == k).ok_or(format!("no power tool {k:?} loaded"))?;
        g.power_fixed_update(p);
        VOID
    }),
    ("PushPowerTool.CreateEffectObject", power_create_effect),
    ("PullPowerTool.CreateEffectObject", power_create_effect),
    ("LiftPowerTool.CreateEffectObject", power_create_effect),
    ("AOEPowerTool.Awake", |g, c| { c.with(0)?; let p = g.power_awake(c.self_kind(crate::game::power::PowerKind::of_tool)?); Ok(format!(" teff={},1,{}", g.powers[p].root, bit(g.powers[p].playing))) }),
    ("FixedWireTool.OnToolChosen", wire_tool_chosen),
    ("DistanceWireTool.OnToolChosen", wire_tool_chosen),
    // gapA r6: the wire tools with an OnToolChosen of their own (base.OnToolChosen, then their line's caps/texture/order, which
    // WireKind::tool_line holds; UseWireTool's _GlowColour block is no tline field): wire_tool_chosen, as their base's.
    ("BandageWireTool.OnToolChosen", wire_tool_chosen),
    ("WoodStrutTool.OnToolChosen", wire_tool_chosen),
    ("UseWireTool.OnToolChosen", wire_tool_chosen),
    ("SliderTool.OnToolChosen", wire_tool_chosen),
    // gapA r6: BloodContainer.AllowsTransfer is virtual (true; ValveBehaviour's ValvePosition > 0.5, LiquidPressureValveBehaviour's
    // and LiquidPumpBehaviour's false): wire_allows on the self's owner. LiquidPressureValveBehaviour.AllowPressureTransfer:
    // ValvePosition > 0.5 (a pressure_only valve).
    ("BloodContainer.get_AllowsTransfer", |g, c| { let o = c.with(1)?.p_owner(g, 0)?; Ok(Enc::enc(g.wire_allows(o))) }),
    ("LiquidPressureValveBehaviour.get_AllowsTransfer", |g, c| { let o = c.with(1)?.p_owner(g, 0)?; Ok(Enc::enc(g.wire_allows(o))) }),
    ("LiquidPressureValveBehaviour.get_AllowPressureTransfer", |g, c| { let o = c.with(1)?.p_owner(g, 0)?; Ok(Enc::enc(g.container_allows_pressure(o))) }),
    // gapA r6: ExistInThermalVision.ThermalVisionToggled on its own object (the generated entry ran Start, which also adds the listener).
    ("ExistInThermalVision.ThermalVisionToggled", |g, c| { c.with(1)?; g.exist_in_thermal_vision_toggled(c.sid); VOID }),
    ("BloodWireTool.OnJointCreate", |g, c| {
        c.with(1)?;
        let f = c.adopted_joint(g, 0).ok_or("joint arg: not adopted")?;
        let kind = super::super::blood_wire::WireKind::of_tool("BloodWireTool").ok_or("no BloodWireTool kind")?;
        g.use_wire_tool_on_joint_create(f, kind);
        VOID
    }),
    // fnrec r1: the other wire tools' OnJointCreate(joint) on the recorder's MakeArg DistanceJoint2D / SpringJoint2D (fnweap::load_wires
    // adopts it), as BloodWireTool's and SpringWireTool's: the shared body with the self class's kind.
    ("RopeTool.OnJointCreate", wire_tool_on_joint_create),
    ("CableWireTool.OnJointCreate", wire_tool_on_joint_create),
    ("ChainWireTool.OnJointCreate", wire_tool_on_joint_create),
    ("CopperWireTool.OnJointCreate", wire_tool_on_joint_create),
    ("EnergyWireTool.OnJointCreate", wire_tool_on_joint_create),
    ("HeatPipeTool.OnJointCreate", wire_tool_on_joint_create),
    ("RigidWireTool.OnJointCreate", wire_tool_on_joint_create),
    ("WoodStrutTool.OnJointCreate", wire_tool_on_joint_create),
    ("BandageWireTool.OnJointCreate", wire_tool_on_joint_create),
    ("StrongSpringWireTool.OnJointCreate", wire_tool_on_joint_create),
    ("DestructibleSpringWireTool.OnJointCreate", wire_tool_on_joint_create),
    // fnrec r1: the fixed wire tools' OnJointCreate(joint, worldSpaceEndpos) on the recorder's MakeArg FixedJoint2D (fnweap::load_wires
    // adopts it): the shared body with the self class's kind, then localRenderEndpoint from the end point.
    ("FixedCableTool.OnJointCreate", fixed_wire_tool_on_joint_create),
    ("SteelBindingTool.OnJointCreate", fixed_wire_tool_on_joint_create),
    ("WoodenBindingTool.OnJointCreate", fixed_wire_tool_on_joint_create),
    ("CopperWireBehaviour.JointBroken", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.wire_joint_broken(w); VOID }),
    ("CopperWireBehaviour.SetWireColours", |g, c| { c.with(1)?; VOID }),
    ("CopperWireBehaviour.Start", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.blood_wires[w].started = false; g.blood_wire_start(w); VOID }),
    // fnweap: FixedJointWireBehaviour (a FixedCableBehaviour). CalculateEdgeCollider: EdgeCollider is always null.
    ("SteelBindingBehaviour.Tick", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.binding_tick(w); VOID }),
    ("SteelBindingBehaviour.Slice", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.wire_slice(w); VOID }),
    ("SteelBindingBehaviour.Start", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.blood_wires[w].started = false; g.blood_wire_start(w); VOID }),
    ("FixedCableBehaviour.Start", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.blood_wires[w].started = false; g.blood_wire_start(w); VOID }),
    ("FixedJointWireBehaviour.Update", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.blood_wire_update(w); VOID }),
    ("FixedJointWireBehaviour.OnPreFlip", |g, c| { let c = c.with(2)?; let w = c.p_usize(g, 0)?; g.blood_wire_on_pre_flip(w); VOID }),
    // OnPreFlip(Vector3 center): the generated entry takes no center.
    ("DistanceJointWireBehaviour.OnPreFlip", |g, c| { let c = c.with(2)?; let w = c.p_usize(g, 0)?; g.blood_wire_on_pre_flip(w); VOID }),
    ("FixedJointWireBehaviour.OnPlayerDragStart", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.blood_wires[w].hasMovedInPauseMode = true; VOID }),
    ("FixedJointWireBehaviour.OnPlayerDragEnd", |g, c| { let c = c.with(1)?; let w = c.p_usize(g, 0)?; g.blood_wires[w].hasMovedInPauseMode = true; VOID }),
    // gapB r8: ExplosionSoundBehviour.Play(delay): the self's prefab fields as link.rs's EventCall::ExplosionSoundPlay reads them.
    ("ExplosionSoundBehviour.Play", |g, c| {
        let c = c.with(1)?;
        let (id, delay) = (c.sid(), c.p_f32(0)?);
        let (enabled, has, near, distant) = g.with_component_instance(id, |_, inst, o| {
            let k = inst.objects[o].components.iter().position(|&x| x == id).unwrap();
            let comps = &inst.prefab.objects[o].components;
            let crate::assets::db::Component::Script { enabled, fields, .. } = &comps[k].1 else { return Err("ExplosionSoundBehviour: no script".to_string()) };
            use crate::assets::mono::Value;
            let has = fields.get("audioSource").and_then(Value::pptr).is_some_and(|p| !p.is_null()) || comps.iter().any(|(_, x)| matches!(x, crate::assets::db::Component::AudioSource { .. }));
            Ok((*enabled, has, fields.get("Near").and_then(Value::array).map_or(0, |a| a.len()), fields.get("DistantSound").and_then(Value::i64).unwrap_or(0)))
        })?;
        if !g.destroyed_components.contains(&id) && g.explosion_sound_enabled(id, enabled) {
            g.explosion_sound_play(id, has.then_some(near), distant, delay);
        }
        VOID
    }),
    // FixedJointWireBehaviour.Awake: WireBehaviour.Awake's GetComponent<PhysicalBehaviour> (the same one) and event subscriptions only.
    ("FixedJointWireBehaviour.Awake", |g, c| { c.with(1)?; VOID }),
    ("FixedJointWireBehaviour.OnUserDelete", |g, c| { c.with(1)?; VOID }),
    ("FixedJointWireBehaviour.CalculateEdgeCollider", |g, c| { c.with(1)?; crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object") }),
    // fnweap: the belt's own functions.
    ("SerpentineBeltBehaviour.IsMouseInsideCollider", |g, c| { let c = c.with(1)?; let l = c.p_usize(g, 0)?; Ok(bit(g.belt_is_mouse_inside_collider(l))) }),
    ("SerpentineBeltBehaviour.GetVisualBounds", |g, c| {
        let c = c.with(1)?;
        let l = c.p_usize(g, 0)?;
        Ok(match g.belt_get_visual_bounds(l) { Ok((cn, e)) => format!("Bounds(m_Center={} m_Extents={})", vec(cn), vec(e)), Err(e) => format!("exc:{e}") })
    }),
    ("SerpentineBeltBehaviour.OnDestroy", |g, c| { let c = c.with(1)?; let l = c.p_usize(g, 0)?; g.belt_on_destroy(l); VOID }),
    ("SerpentineBeltBehaviour.get_ShouldEnableRenderer", |g, c| { let c = c.with(1)?; let l = c.p_usize(g, 0)?; Ok(bit(g.belt_should_enable_renderer(l))) }),
    // Awake: a new line renderer (its colours and widths are presentation). OnUserDelete: empty.
    ("SerpentineBeltBehaviour.Awake", |g, c| { let c = c.with(1)?; let l = c.p_usize(g, 0)?; g.belts[l].line_renderer = true; VOID }),
    ("SerpentineBeltBehaviour.OnUserDelete", |g, c| { c.with(1)?; VOID }),
    ("LimbBehaviour.CalculateJointStress", |g, c| { g.calculate_joint_stress(c.limb()?); VOID }),
    ("LimbBehaviour.SetMotorStrength", |g, c| { g.set_motor_strength(c.limb()?, c.f(0)); VOID }),
    ("LimbBehaviour.SetMotorStrengthToMuscleStrength", |g, c| { g.set_motor_strength_to_muscle_strength(c.limb()?); VOID }),
    ("LimbBehaviour.SetJointFragility", |g, c| { g.set_joint_fragility(c.limb()?); VOID }),
    ("LimbBehaviour.IsWorldPointInVitalPart", |g, c| Ok(bit(g.is_world_point_in_vital_part(c.limb()?, c.v(0))))),
    ("LimbBehaviour.HandleDrowning", |g, c| { g.handle_drowning(c.limb()?); VOID }),
    ("LimbBehaviour.FakeStandUpright", |g, c| { g.fake_stand_upright(c.limb()?); VOID }),
    ("LimbBehaviour.get_IsOnFloor", |g, c| Ok(bit(g.limbs[c.limb()?].IsOnFloor))),
    ("LimbBehaviour.set_IsOnFloor", |g, c| { g.limbs[c.limb()?].IsOnFloor = c.args[0] == "1"; VOID }),
    ("LimbBehaviour.get_JointStress", |g, c| Ok(hex(g.limbs[c.limb()?].JointStress))),
    ("LimbBehaviour.set_JointStress", |g, c| { g.limbs[c.limb()?].JointStress = c.f(0); VOID }),
    ("LimbBehaviour.Damage", |g, c| { g.limb_damage(c.limb()?, c.f(0)); VOID }),
    ("CirculationBehaviour.GetHeartRate", |g, c| Ok(hex(g.heart_rate(c.circ()?)))),
    ("CirculationBehaviour.IsWorldPointInArteryRect", |g, c| Ok(bit(g.is_world_point_in_artery_rect(c.circ()?, c.v(0))))),
    ("CirculationBehaviour.PumpBehaviour", |g, c| { g.pump_behaviour(c.circ()?, c.f(0)); VOID }),
    ("CirculationBehaviour.get_Limits", |g, c| Ok(vec(g.container_limits(Owner::Circulation(c.circ()?))))),
    ("CirculationBehaviour.HealBleeding", |g, c| { g.heal_bleeding(c.circ()?); VOID }),
    ("CirculationBehaviour.Cut", |g, c| { g.cut(c.circ()?, c.v(0), c.v(1)); VOID }),
    ("BloodContainer.get_Limits", |g, c| Ok(vec(g.container_limits(Owner::Circulation(c.circ()?))))),
    ("BloodContainer.GetComputedColor", |g, c| Ok(vec(g.get_computed_color(Owner::Circulation(c.circ()?), c.v(0))))),
    ("BloodContainer.RemoveLiquid", |g, c| {
        let liquid = super::super::circulation::liquid(&c.args[0].replace('_', " "));
        Ok(hex(g.remove_liquid(Owner::Circulation(c.circ()?), liquid, c.f(1))))
    }),
    ("BloodContainer.get_BloodAmount", |g, c| Ok(hex(g.circulations[c.circ()?].base.TotalLiquidAmount))),
    ("BloodContainer.set_BloodAmount", |g, c| { g.force_set_all_liquid(Owner::Circulation(c.circ()?), c.f(0)); VOID }),
    ("BloodContainer.get_TotalLiquidAmount", |g, c| Ok(hex(g.circulations[c.circ()?].base.TotalLiquidAmount))),
    ("BloodContainer.set_TotalLiquidAmount", |g, c| { g.circulations[c.circ()?].base.TotalLiquidAmount = c.f(0); VOID }),
    ("BloodContainer.get_AllowPressureTransfer", |g, c| Ok(bit(g.container_allows_pressure(Owner::Circulation(c.circ()?))))),
    ("BloodContainer.get_AllowsOverflow", |g, c| Ok(bit(g.allows_overflow(Owner::Circulation(c.circ()?))))),
    ("BloodContainer.get_IsComputerColourCached", |g, c| Ok(bit(g.circulations[c.circ()?].base.isComputedColourCached))),
    ("PersonBehaviour.IsAlive", |g, c| Ok(bit(g.person_is_alive(c.index("PersonBehaviour")?)))),
    ("PersonBehaviour.get_IsHeadAlive", |g, c| Ok(bit(g.person_is_alive(c.index("PersonBehaviour")?)))),
    ("PersonBehaviour.HandleDeathCounter", |g, c| { g.handle_death_counter(c.index("PersonBehaviour")?); VOID }),
    ("PersonBehaviour.AttachedLimbLoopChecks", |g, c| { g.attached_limb_loop_checks(c.index("PersonBehaviour")?); VOID }),
    ("PhysicalBehaviour.GetHeatCapacity", |g, c| Ok(hex(g.heat_capacity(c.phys(g)?)))),
    ("PhysicalBehaviour.IsPointNearLooseSharpEnd", |g, c| Ok(bit(g.is_point_near_loose_sharp_end(c.phys(g)?, c.v(0))))),
    ("PhysicalBehaviour.GetNearestLocalHoldingPoint", |g, c| {
        let (p, d) = g.nearest_local_holding_point(c.phys(g)?, c.v(0));
        Ok(format!("{} out={};{}", vec(p), c.args[0], hex(d)))
    }),
    ("PhysicalBehaviour.GetRandomGridPoint", |g, c| Ok(vec(g.random_grid_point(c.phys(g)?)))),
    ("Utils.AverageTemperature", |g, c| {
        let other = reference(c.args[1]).ok_or("no second PhysicalBehaviour")?;
        let (a, b) = (c.phys(g)?, phys_by_id(g, other)?);
        g.average_temperature(a, b, c.f(2));
        VOID
    }),
    // Batch 2 (tools/fnscale).
    // Each Liquid class's OnEnterLimb override (self: the class's registry liquid), through Liquid.OnEnterLimb's virtual call.
    // A liquid's constant properties, on the registered liquid of the class.
    ("CirculationPoison.get_ShouldCallOnEnterEveryUpdate", |_, c| { let _ = c.with(0)?; Ok(Enc::enc(super::super::syringe::should_call_on_enter_every_update("CirculationPoison"))) }),
    ("DurabilitySerum.get_ShouldCallOnEnterEveryUpdate", |_, c| { let _ = c.with(0)?; Ok(Enc::enc(super::super::syringe::should_call_on_enter_every_update("DurabilitySerum"))) }),
    ("ExplosionPoison.get_ShouldCallOnEnterEveryUpdate", |_, c| { let _ = c.with(0)?; Ok(Enc::enc(super::super::syringe::should_call_on_enter_every_update("ExplosionPoison"))) }),
    ("MassManipulationPoison.get_ShouldCallOnEnterEveryUpdate", |_, c| { let _ = c.with(0)?; Ok(Enc::enc(super::super::syringe::should_call_on_enter_every_update("MassManipulationPoison"))) }),
    ("MusclePoison.get_ShouldCallOnEnterEveryUpdate", |_, c| { let _ = c.with(0)?; Ok(Enc::enc(super::super::syringe::should_call_on_enter_every_update("MusclePoison"))) }),
    ("ReflectionPoison.get_ShouldCallOnEnterEveryUpdate", |_, c| { let _ = c.with(0)?; Ok(Enc::enc(super::super::syringe::should_call_on_enter_every_update("ReflectionPoison"))) }),
    ("TransparencyPoison.get_ShouldCallOnEnterEveryUpdate", |_, c| { let _ = c.with(0)?; Ok(Enc::enc(super::super::syringe::should_call_on_enter_every_update("TransparencyPoison"))) }),
    ("OsteomorphosisAgent.get_RemovalChancePerSecond", |_, c| { let _ = c.with(0)?; Ok(Enc::enc(super::super::syringe::osteomorphosis_agent_removal_chance_per_second())) }),
    // ExoticLiquid.OnEnterContainer(container): the container is unused.
    ("ExoticLiquid.OnEnterContainer", |g, c| { let _ = c.with(1)?; g.exotic_liquid_on_enter_container(); VOID }),
    ("AdrenalineLiquid.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("BoneHurtingJuiceLiquid.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("CirculationPoison.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("CoagulationLiquid.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("CombustionAgent.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("CrushingPoison.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("DurabilitySerum.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("ExplosionPoison.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("FreezePoisonLiquid.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("GorseBlood.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("InstantDeathPoisonLiquid.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("KnockoutPoisonLiquid.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("MassManipulationPoison.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("MusclePoison.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("NumbingPoison.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("PainKillerLiquid.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("PinkDormantLiquid.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("ReflectionPoison.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("RegenerationSerum.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("SizeManipulationPoison.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("TissueDeconstructionAgent.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("TransparencyPoison.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("WaterBreathingSerum.OnEnterLimb", |g, c| { g.liquid_on_enter_limb(c.liq()?, c.arg_index(0, "LimbBehaviour")?); VOID }),
    ("LimbBehaviour.ManagedFixedUpdate", |g, c| { g.limb_fixed_update(c.limb()?); VOID }),
    ("LimbBehaviour.ManagedUpdate", |g, c| { g.limb_update(c.limb()?); VOID }),
    ("LimbBehaviour.get_MotorStrength", |g, c| Ok(hex(g.motor_strength(c.limb()?)))),
    ("LimbBehaviour.get_IsCapable", |g, c| Ok(bit(g.limb_is_capable(c.limb()?)))),
    ("LimbBehaviour.get_IsConsideredAlive", |g, c| Ok(bit(g.limb_is_considered_alive(c.limb()?)))),
    ("LimbBehaviour.get_IsParalysed", |g, c| Ok(bit(g.limb_is_paralysed(c.limb()?)))),
    ("LimbBehaviour.GetMassStrengthRatio", |g, c| Ok(hex(g.mass_strength_ratio(c.limb()?)))),
    ("LimbBehaviour.IsAnyHeadAlive", |g, c| Ok(bit(g.is_any_head_alive(c.limb()?)))),
    ("LimbBehaviour.ApplyPoseOverrides", |g, c| Ok(bit(g.apply_pose_overrides(c.limb()?)))),
    ("LimbBehaviour.InfluenceMotorSpeed", |g, c| { g.influence_motor_speed(c.limb()?, c.f(0), c.f(1)); VOID }),
    ("LimbBehaviour.WaterImpact", |g, c| { g.limb_water_impact(c.limb()?, c.f(0)); VOID }),
    ("LimbBehaviour.Wince", |g, c| { g.limb_wince(c.limb()?, c.f(0)); VOID }),
    ("LimbBehaviour.StunImpact", |g, c| { g.limb_stun_impact(c.limb()?); VOID }),
    ("LimbBehaviour.ActOnImpact", |g, c| { g.act_on_impact(c.limb()?, c.f(0), c.v(1)); VOID }),
    ("LimbBehaviour.PropagateImpactDamage", |g, c| {
        let origin = c.arg_index(4, "LimbBehaviour")?;
        g.propagate_impact_damage(c.limb()?, c.f(0), c.v(1), c.v(2), c.n(3)?, origin);
        VOID
    }),
    // fc41: the private color and the SpriteRenderer colour, before (" stpre=lc:") and after (" st=lc:") the call (FnTiny.StateOf).
    ("LimbBehaviour.set_Color", |g, c| {
        let l = c.limb()?;
        let p = g.limbs[l].PhysicalBehaviour;
        let (color, sprite) = st_pre(c).and_then(|s| s.strip_prefix("lc:")?.split_once('|')).ok_or("no lc: stpre")?;
        g.limbs[l].Color = floats(color);
        g.physicals[p].sprite.as_mut().ok_or("a limb without a SpriteRenderer")?.1 = floats(sprite);
        g.limb_set_color(l, c.v(0));
        let hex = |v: [f32; 4]| v.map(|x| format!("{:08x}", x.to_bits())).join(",");
        Ok(format!(" st=lc:{}|{}", hex(g.limbs[l].Color), hex(g.physicals[p].sprite.as_ref().ok_or("a limb without a SpriteRenderer")?.1)))
    }),
    ("LimbBehaviour.BreakBone", |g, c| { g.break_bone(c.limb()?); VOID }),
    ("LimbBehaviour.BreakBoneInternal", |g, c| { g.break_bone_internal(c.limb()?); VOID }),
    ("LimbBehaviour.HealBone", |g, c| { g.limb_heal_bone(c.limb()?); VOID }),
    ("LimbBehaviour.Slice", |g, c| { g.limb_slice(c.limb()?); VOID }),
    ("LimbBehaviour.Crush", |g, c| { g.limb_crush(c.limb()?); VOID }),
    ("CirculationBehaviour.FixedUpdate", |g, c| { g.circulation_fixed_update(c.circ()?); VOID }),
    ("CirculationBehaviour.HandleDamageEdgeCases", |g, c| { g.handle_damage_edge_cases(c.circ()?); VOID }),
    ("CirculationBehaviour.HandleBleeding", |g, c| { g.handle_bleeding(c.circ()?, c.f(0)); VOID }),
    ("CirculationBehaviour.HandlePenetrationBleeding", |g, c| { g.handle_penetration_bleeding(c.circ()?); VOID }),
    ("CirculationBehaviour.FlowStep", |g, c| { g.flow_step(c.circ()?); VOID }),
    ("CirculationBehaviour.DiffuseStep", |g, c| { g.diffuse_step(c.circ()?); VOID }),
    ("CirculationBehaviour.DiffuseToLimb", |g, c| { g.diffuse_to_limb(c.circ()?, c.arg_index(0, "LimbBehaviour")?, c.f(1)); VOID }),
    ("CirculationBehaviour.get_IsConnectedToMainBody", |g, c| Ok(bit(g.is_connected_to_main_body(c.circ()?)))),
    ("CirculationBehaviour.get_HasCirculation", |g, c| Ok(bit(g.has_circulation(c.circ()?)))),
    ("CirculationBehaviour.get_HasBloodFlow", |g, c| Ok(bit(g.has_blood_flow(c.circ()?)))),
    ("CirculationBehaviour.GetAmountOfBlood", |g, c| Ok(hex(g.amount_of_blood(c.circ()?)))),
    ("CirculationBehaviour.OnLiquidEnter", |g, c| { g.on_liquid_enter(c.circ_owner()?, c.liquid(0)?); VOID }),
    ("CirculationBehaviour.CreateBleedingParticle", |g, c| { g.create_bleeding_particle(c.circ()?, c.v(0), c.v(1), c.f(2)); VOID }),
    ("CirculationBehaviour.CreateDismembermentBloodParticle", |g, c| { g.create_dismemberment_blood_particle(c.circ()?); VOID }),
    ("CirculationBehaviour.Disintegrate", |g, c| { g.circulation_disintegrate(c.circ()?); VOID }),
    ("BloodContainer.AddLiquid", |g, c| Ok(hex(g.add_liquid(c.circ_owner()?, c.liquid(0)?, c.f(1))))),
    ("BloodContainer.Drain", |g, c| { g.drain(c.circ_owner()?, c.f(0)); VOID }),
    ("BloodContainer.DeleteEmptyLiquidEntries", |g, c| { g.delete_empty_liquid_entries(c.circ_owner()?); VOID }),
    ("BloodContainer.ForceSetAllLiquid", |g, c| { g.force_set_all_liquid(c.circ_owner()?, c.f(0)); VOID }),
    ("BloodContainer.ClearLiquid", |g, c| { g.clear_liquid(c.circ_owner()?); VOID }),
    // fnmaped r15: the self is the container (the generated entry reads the List<GameObject> arg as it); MapConfigProbe.Seed
    // fills SerialisableDistributions with k % 4 registered liquids.
    ("BloodContainer.OnAfterDeserialise", |g, c| { g.blood_container_on_after_deserialise(c.circ_owner()?); VOID }),
    ("BloodContainer.ForceRecalculateTotalLiquidAmount", |g, c| { g.force_recalculate_total_liquid_amount(c.circ_owner()?); VOID }),
    ("BloodContainer.TransferTo", |g, c| {
        let target = Owner::Circulation(c.arg_index(2, "CirculationBehaviour")?);
        Ok(hex(g.transfer_liquid_to(c.circ_owner()?, c.liquid(0)?, c.f(1), target)))
    }),
    ("BloodContainer.CopyTo", |g, c| {
        let target = Owner::Circulation(c.arg_index(1, "CirculationBehaviour")?);
        g.copy_to(c.circ_owner()?, c.f(0), target, c.b(2));
        VOID
    }),
    ("BloodContainer.GetAmount", |g, c| Ok(hex(g.get_amount(c.circ_owner()?, c.liquid(0)?)))),
    ("BloodContainer.GetPercentageOf", |g, c| Ok(hex(g.get_percentage_of(c.circ_owner()?, c.liquid(0)?)))),
    ("BloodContainer.Update", |g, c| { g.blood_container_update(c.circ_owner()?); VOID }),
    ("BloodContainer.ForceCalculateComputedColor", |g, c| Ok(vec(g.force_calculate_computed_color(c.circ_owner()?, c.v(0))))),
    ("BloodContainer.IsFull", |g, c| Ok(bit(g.is_full(c.circ_owner()?, c.f(0))))),
    ("BloodContainer.get_LowerLimit", |g, c| Ok(hex(g.lower_limit(c.circ_owner()?)))),
    ("BloodContainer.get_UpperLimit", |g, c| Ok(hex(g.upper_limit(c.circ_owner()?)))),
    ("BloodContainer.get_ScaledLiquidAmount", |g, c| Ok(hex(g.scaled_liquid_amount(c.circ_owner()?)))),
    // fc41 r10: the self's GlowInTheDarkBehaviour from its PI Clone (FnTiny's Tritium cases add one; Tritium's exit destroys it).
    ("BloodContainer.OnLiquidExit", |g, c| { let o = c.circ_owner()?; glow_pre_on(g, c, o, &c.id.to_string()); g.on_liquid_exit(o, c.liquid(0)?); VOID }),
    ("PersonBehaviour.Update", |g, c| { g.person_update(c.index("PersonBehaviour")?); VOID }),
    ("PersonBehaviour.FixedUpdate", |g, c| { g.person_fixed_update(c.index("PersonBehaviour")?); VOID }),
    ("PersonBehaviour.LateUpdate", |g, c| { g.person_late_update(c.index("PersonBehaviour")?); VOID }),
    ("PersonBehaviour.DetermineActivePose", |g, c| { g.person_late_update(c.index("PersonBehaviour")?); VOID }),
    ("PersonBehaviour.AllLimbLoopChecks", |g, c| { g.all_limb_loop_checks(c.index("PersonBehaviour")?); VOID }),
    ("PersonBehaviour.AddPain", |g, c| { g.add_pain(c.index("PersonBehaviour")?, c.f(0)); VOID }),
    ("PersonBehaviour.Wince", |g, c| { g.person_wince(c.index("PersonBehaviour")?, c.f(0)); VOID }),
    ("PersonBehaviour.SetBruiseColor", |g, c| person_colour(g, c, |g, p| { g.person_set_bruise_color(p, c.n(0)?, c.n(1)?, c.n(2)?); Ok(()) })),
    ("PersonBehaviour.SetSecondBruiseColor", |g, c| person_colour(g, c, |g, p| { g.person_set_second_bruise_color(p, c.n(0)?, c.n(1)?, c.n(2)?); Ok(()) })),
    ("PersonBehaviour.SetThirdBruiseColor", |g, c| person_colour(g, c, |g, p| { g.person_set_third_bruise_color(p, c.n(0)?, c.n(1)?, c.n(2)?); Ok(()) })),
    ("PersonBehaviour.SetBloodColour", |g, c| person_colour(g, c, |g, p| { g.person_set_blood_colour(p, c.n(0)?, c.n(1)?, c.n(2)?); Ok(()) })),
    ("PersonBehaviour.SetRottenColour", |g, c| person_colour(g, c, |g, p| { g.person_set_rotten_colour(p, c.n(0)?, c.n(1)?, c.n(2)?); Ok(()) })),
    ("PersonBehaviour.SetShaderColour", |g, c| person_colour(g, c, |g, p| { g.person_set_shader_colour(p, c.arg_str(0)?, c.n(1)?, c.n(2)?, c.n(3)?); Ok(()) })),
    ("PhysicalBehaviour.ManagedUpdate", |g, c| { g.physical_update(c.phys(g)?); VOID }),
    ("PhysicalBehaviour.ManagedLateUpdate", |g, c| { g.physical_late_update(c.phys(g)?); VOID }),
    ("PhysicalBehaviour.ManagedFixedUpdate", |g, c| { g.physical_fixed_update(c.phys(g)?); VOID }),
    ("PhysicalBehaviour.ShouldUpdate", |g, c| Ok(bit(g.physical_should_update(c.phys(g)?)))),
    ("PhysicalBehaviour.get_IsTouchingSomething", |g, c| Ok(bit(g.is_touching_something(c.phys(g)?)))),
    ("PhysicalBehaviour.get_IsBeingStabbed", |g, c| Ok(bit(g.is_being_stabbed(c.phys(g)?)))),
    ("PhysicalBehaviour.get_LocalGridPointLength", |g, c| Ok(g.local_grid_point_length(c.phys(g)?).to_string())),
    ("PhysicalBehaviour.get_Charge", |g, c| {
        let p = &g.physicals[c.phys(g)?];
        Ok(hex(if p.ForceNoCharge { 0.0 } else { p.charge }))
    }),
    ("PhysicalBehaviour.get_BurnIntensity", |g, c| Ok(hex(g.physicals[c.phys(g)?].burnIntensity))),
    ("PhysicalBehaviour.get_ShowOutline", |g, c| Ok(bit(g.physicals[c.phys(g)?].showOutline))),
    ("PhysicalBehaviour.GetPreviousVel", |g, c| Ok(vec(g.physical_previous_vel(c.phys(g)?)))),
    ("PhysicalBehaviour.GetChargeWithWireResistance", |g, c| Ok(hex(g.physicals[c.phys(g)?].GetChargeWithWireResistance()))),
    ("PhysicalBehaviour.GetLowestGlobalGridPoint", |g, c| Ok(vec(g.physical_lowest_grid_point(c.phys(g)?)))),
    ("PhysicalBehaviour.GetClosestPointTo", |g, c| {
        let [x, y] = c.v(0);
        let p = c.phys(g)?;
        Ok(vec(g.physical_closest_point(p, crate::b2::math::Vec2::new(x, y))))
    }),
    ("PhysicalBehaviour.GetWorldCenterOfMass", |g, c| {
        let v = g.physical_world_center_of_mass(c.phys(g)?);
        Ok(vec([v[0], v[1]]))
    }),
    ("PhysicalBehaviour.GetRelativeStabSpeed", |g, c| {
        let (p, v) = (c.phys(g)?, c.arg_phys(g, 0)?);
        Ok(hex(g.get_relative_stab_speed(p, v)))
    }),
    ("PhysicalBehaviour.IsBeingUsedContinuously", |g, c| Ok(bit(g.used_continuously_on(c.phys(g)?, c.n(0)?)))),
    ("PhysicalBehaviour.WasBeingUsedContinuously", |g, c| Ok(bit(g.was_used_continuously_on(c.phys(g)?, c.n(0)?)))),
    ("PhysicalBehaviour.StartedBeingUsedContinuously", |g, c| Ok(bit(g.started_used_continuously(c.phys(g)?)))),
    ("PhysicalBehaviour.StoppedBeingUsedContinuously", |g, c| Ok(bit(g.stopped_used_continuously(c.phys(g)?)))),
    ("PhysicalBehaviour.HaltPropagation", |g, c| { g.physical_halt_propagation(c.phys(g)?); VOID }),
    ("PhysicalBehaviour.ClearAllDecals", |g, c| { g.physical_clear_all_decals(c.phys(g)?); VOID }),
    ("PhysicalBehaviour.CalculateCircumference", |g, c| { g.physical_calculate_circumference(c.phys(g)?); VOID }),
    ("PhysicalBehaviour.RecalculateMassBasedOnSize", |g, c| { g.physical_recalculate_mass_based_on_size(c.phys(g)?); VOID }),
    ("PhysicalBehaviour.MakeWeightless", |g, c| { g.physical_make_weightless(c.phys(g)?); VOID }),
    ("PhysicalBehaviour.MakeWeightful", |g, c| { g.physical_make_weightful(c.phys(g)?); VOID }),
    ("PhysicalBehaviour.Extinguish", |g, c| { g.extinguish(c.phys(g)?); VOID }),
    ("PhysicalBehaviour.Sizzle", |g, c| { g.sizzle(c.phys(g)?, c.b(0)); VOID }),
    ("PhysicalBehaviour.IgniteInternal", |g, c| { g.ignite_internal(c.phys(g)?, c.b(0)); VOID }),
    ("PhysicalBehaviour.AffectSurroundings", |g, c| { g.affect_surroundings(c.phys(g)?); VOID }),
    ("PhysicalBehaviour.OnCollisionStayWithoutSleep", |g, c| { g.on_collision_stay_without_sleep(c.phys(g)?); VOID }),
    ("PhysicalBehaviour.HandleSlidingSounds", |g, c| { g.handle_sliding_sounds(c.phys(g)?); VOID }),
    ("PhysicalBehaviour.SetProperties", |g, c| { g.physical_set_properties(c.phys(g)?); VOID }),
    ("PhysicalBehaviour.Disintegrate", |g, c| { g.physical_disintegrate(c.phys(g)?); VOID }),
    ("GForceMeasureBehaviour.FixedUpdate", |g, c| { g.gforce_fixed_update(c.comp("GForceMeasureBehaviour")?); VOID }),
    ("GForceMeasureBehaviour.get_VelocityLastFrame", |g, c| Ok(vec(g.gforces[c.comp("GForceMeasureBehaviour")?].velocityLastFrame))),
    ("GForceMeasureBehaviour.get_Acceleration", |g, c| Ok(vec(g.gforces[c.comp("GForceMeasureBehaviour")?].acceleration))),
    ("GForceMeasureBehaviour.get_SustainedAcceleration", |g, c| Ok(vec(g.gforces[c.comp("GForceMeasureBehaviour")?].sustainedAcceleration))),
    ("SkinMaterialHandler.Update", |g, c| { g.skin_update(c.comp("SkinMaterialHandler")?); VOID }),
    ("SkinMaterialHandler.LateUpdate", |g, c| { g.skin_late_update(c.comp("SkinMaterialHandler")?); VOID }),
    ("SkinMaterialHandler.RegenerateWounds", |g, c| Ok(bit(g.regenerate_wounds(c.comp("SkinMaterialHandler")?, c.f(0))))),
    ("SkinMaterialHandler.AddDamagePoint", |g, c| { g.skin_add_damage_point(c.comp("SkinMaterialHandler")?, c.n(0)?, c.v(1), c.f(2)); VOID }),
    ("SkinMaterialHandler.InternalAddDamagePoint", |g, c| { g.skin_internal_add_damage_point(c.comp("SkinMaterialHandler")?, c.n(0)?, c.v(1), c.f(2)); VOID }),
    ("SkinMaterialHandler.ImpactDamage", |g, c| {
        let [x, y, _]: [f32; 3] = c.v(1);
        g.skin_impact_damage(c.comp("SkinMaterialHandler")?, c.f(0), [x, y]);
        VOID
    }),
    ("SkinMaterialHandler.ShotDamage", |g, c| {
        let [x, y, _]: [f32; 3] = c.v(1);
        g.skin_shot_damage(c.comp("SkinMaterialHandler")?, c.f(0), [x, y]);
        VOID
    }),
    ("SkinMaterialHandler.ClearAllDamage", |g, c| { g.skin_clear_all_damage(c.comp("SkinMaterialHandler")?); VOID }),
    ("SkinMaterialHandler.get_IsSkinVisible", |g, c| Ok(bit(g.skin_is_skin_visible(c.comp("SkinMaterialHandler")?)))),
    ("SkinMaterialHandler.get_IsFleshVisible", |g, c| Ok(bit(g.skin_is_flesh_visible(c.comp("SkinMaterialHandler")?)))),
    ("SkinMaterialHandler.get_RottenProgress", |g, c| Ok(hex(g.skins[c.comp("SkinMaterialHandler")?].rottenProgress))),
    ("SkinMaterialHandler.get_AcidProgress", |g, c| Ok(hex(g.skins[c.comp("SkinMaterialHandler")?].acidProgress))),
    ("SkinMaterialHandler.set_RottenProgress", |g, c| {
        let s = c.comp("SkinMaterialHandler")?;
        g.skins[s].set_rotten(c.f(0));
        VOID
    }),
    ("SkinMaterialHandler.set_AcidProgress", |g, c| {
        let s = c.comp("SkinMaterialHandler")?;
        g.skins[s].set_acid(c.f(0));
        VOID
    }),
    ("GoreStringBehaviour.Update", |g, c| { g.gore_string_update(c.gore_limb()?); VOID }),
    ("GoreStringBehaviour.SetBreakForce", |g, c| { g.gore_string_set_break_force(c.gore_limb()?); VOID }),
    // fc41 r4: OnDestroy on the self's limb (the GEN entry's self index is not a limb).
    ("GoreStringBehaviour.OnDestroy", |g, c| { g.gore_string_on_destroy(c.gore_limb()?); VOID }),
    ("GoreStringBehaviour.TryPlaySnapSound", |g, c| { g.gore_string_snap_sound(c.gore_limb()?); VOID }),
    ("GoreStringBehaviour.DestroyJoint", |g, c| { g.gore_string_destroy_joint(c.gore_limb()?); VOID }),
    ("GoreStringBehaviour.CreateJoint", |g, c| { g.gore_string_create_joint(c.gore_limb()?); VOID }),
    ("ConnectedNodeBehaviour.ResetValue", |g, c| { g.connected_node_reset_value(c.comp("ConnectedNodeBehaviour")?); VOID }),
    ("ConnectedNodeBehaviour.IsConnectedTo", |g, c| Ok(bit(g.node_is_connected_to(c.comp("ConnectedNodeBehaviour")?, c.arg_index(0, "ConnectedNodeBehaviour")?)))),
    ("ConnectedNodeBehaviour.DisconnectFrom", |g, c| { g.node_disconnect_from(c.comp("ConnectedNodeBehaviour")?, c.arg_index(0, "ConnectedNodeBehaviour")?); VOID }),
    ("ConnectedNodeBehaviour.DisconnectFromEverything", |g, c| { g.node_disconnect_from_everything(c.comp("ConnectedNodeBehaviour")?); VOID }),
    // `if (!(joint != Joint))`: another joint (the recorder passes a neighbour's hinge) does nothing.
    ("LimbBehaviour.OnJointBreak2D", |g, c| {
        let l = c.comp("LimbBehaviour")?;
        let own = c.s.of("LimbBehaviour").nth(l).and_then(|r| r.get("Joint")).and_then(reference);
        let arg = reference(c.args.first().ok_or("no joint arg")?);
        if arg.is_some() && arg == own {
            let h = g.limbs[l].Joint.ok_or("a limb Joint without its hinge")?;
            g.limb_on_joint_break(l, h);
            // A call outside a break: SynchroniseDismemberment's Destroy(Joint) is the joint's end (the tail after it never
            // reads the limb's own Joint; a real break destroys the joint itself right after the message).
            g.limb_destroy_joint(l);
        }
        VOID
    }),
    // From the method's start (its IsDismembered return and Destroy(Joint)).
    ("LimbBehaviour.SynchroniseDismemberment", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_opt_usize(g, 1)?; g.synchronise_dismemberment_call(a0, a1); VOID }),
    ("GripBehaviour.Update", |g, c| { g.grip_update(c.comp("GripBehaviour")?); VOID }),
    ("GripBehaviour.FixedUpdate", |g, c| { g.grip_fixed_update(c.comp("GripBehaviour")?); VOID }),
    // fnrec r3: from the recorder's pairs (" stpre=nc:", FnTiny.NcOf: each pair's IgnoreCollisionStackController count and
    // Physics2D.GetIgnoreCollision), the call, then the same pairs after (" st=nc:").
    ("GripBehaviour.RefreshNoCollide", |g, c| {
        let gi = c.comp("GripBehaviour")?;
        let pre = st_pre(c).and_then(|p| p.strip_prefix("nc:"));
        for w in pre.filter(|p| *p != "-").into_iter().flat_map(|p| p.split(';')) {
            let v: Vec<i32> = w.split(':').filter_map(|x| x.parse().ok()).collect();
            let [a, b, n, i] = v[..] else { return Err(format!("bad nc stpre {w}")) };
            if n > 0 { g.ignore_stack.insert((a, b), n); } else { g.ignore_stack.remove(&(a, b)); }
            g.world.ignore_collision(a, b, i == 1);
        }
        g.grip_refresh_no_collide(gi, c.b(0));
        if pre.is_none() { return VOID }
        let Some(held) = g.grip_held(gi) else { return Ok(" st=nc:-".into()) };
        let mut pairs = std::collections::BTreeSet::new();
        for (h, _) in g.colliders_of_held(held) {
            for item in g.grips[gi].CollidersToIgnore.clone() {
                if item != h && g.collider_exists(h) && g.collider_exists(item) {
                    let k = Game::collider_pair(h, item);
                    pairs.insert(format!("{}:{}:{}:{}", k.0, k.1, g.ignore_stack.get(&k).copied().unwrap_or(0), bit(g.world.ignored.contains(&k))));
                }
            }
        }
        Ok(format!(" st=nc:{}", pairs.into_iter().collect::<Vec<_>>().join(";")))
    }),
    ("GripBehaviour.OnChangePause", |g, c| { g.grip_on_change_pause(c.comp("GripBehaviour")?, c.b(0)); VOID }),
    // gapC r17: Awake alone (it was link_tool_choose, which is SetTool's AddComponent + OnToolChosen).
    ("PhaseLinkTool.Awake", |g, c| { c.with(0)?; g.phase_link_tool_awake(); VOID }),
    ("GripBehaviour.PickUpNearestObject", |g, c| { g.grip_pick_up_nearest(c.comp("GripBehaviour")?); VOID }),
    ("GripBehaviour.DropObject", |g, c| { g.grip_drop(c.comp("GripBehaviour")?); VOID }),
    ("Utils.RandomBetween", |g, c| Ok(hex(g.random_between(c.v(0))))),
    ("Utils.TransferEnergyFixedRate", |g, c| {
        let (a, b) = (c.arg_phys(g, 0)?, c.arg_phys(g, 1)?);
        g.transfer_energy_fixed_rate(a, b, c.f(2));
        VOID
    }),
    ("Utils.TransferEnergyDeltaTime", |g, c| {
        let (a, b) = (c.arg_phys(g, 0)?, c.arg_phys(g, 1)?);
        g.transfer_energy_delta_time(a, b, c.f(2), c.f(3));
        VOID
    }),
    // Utils.ClosestPointFix(Rigidbody2D, point): the body's PhysicalBehaviour (record_index maps a Rigidbody2D to it).
    // fnmaped r14: the outlet's OnCollide listeners as MapConfigProbe.ListenPost records them (`lis=<before>,<after>`; Awake's
    // case starts unsubscribed, the probe's OnDestroy first).
    ("LiquidOutletBehaviour.Awake", |g, c| { let l = c.with(1)?.p_usize(g, 0)?; g.liquid_outlet_on_destroy(l); let pre = g.liquid_outlet_listeners(l); g.liquid_outlet_awake(l); Ok(format!("lis={pre},{}", g.liquid_outlet_listeners(l))) }),
    // gapA r7: generated entries the accessor rule (gen_fncheck.py accessor_rule) no longer makes, whose fns are these
    // accessors' own (their docs name the base BloodContainer property first, or the getter's expression); kept by hand.
    ("CirculationBehaviour.get_Limits", |g, c| { let c = c.with(1)?; let a0 = c.p_owner(g, 0)?; Ok(Enc::enc(g.container_limits(a0))) }),
    ("FlaskBehaviour.get_AllowsOverflow", |g, c| { let c = c.with(1)?; let a0 = c.p_owner(g, 0)?; Ok(Enc::enc(g.allows_overflow(a0))) }),
    ("FlaskBehaviour.get_Limits", |g, c| { let c = c.with(1)?; let a0 = c.p_owner(g, 0)?; Ok(Enc::enc(g.container_limits(a0))) }),
    ("PhysicalBehaviour.set_ShowOutline", |g, c| { let c = c.with(2)?; let a0 = c.p_usize(g, 0)?; let a1 = c.p_bool(1)?; g.set_show_outline(a0, a1); VOID }),
    ("ResizeHandles.get_HasTargets", |g, c| { let c = c.with(0)?; Ok(Enc::enc(g.context_resize_has_targets())) }),
    // gapA r7: the OnContinuousUpdate (un)registering Awakes/OnDestroys, as MapConfigProbe.ListenPre/ListenPost record them
    // (`lis=<before>,<after>`: the component's calls on the event; Awake's case starts with them taken out).
    ("KeyTriggerBehaviour.Awake", |g, c| cont_listen(g, c, true, Listener::KeyTrigger)),
    ("LaserReceiverBehaviour.Awake", |g, c| cont_listen(g, c, true, Listener::LaserReceiver)),
    ("LaserReceiverBehaviour.OnDestroy", |g, c| cont_listen(g, c, false, Listener::LaserReceiver)),
    ("LiquidLevelGateBehaviour.Awake", |g, c| cont_listen(g, c, true, Listener::LiquidLevelGate)),
    ("LiquidLevelGateBehaviour.OnDestroy", |g, c| cont_listen(g, c, false, Listener::LiquidLevelGate)),
    ("ActivatorElectrodeBehaviour.Awake", |g, c| cont_listen(g, c, true, Listener::ActivatorElectrode)),
    ("ActivatorElectrodeBehaviour.OnDestroy", |g, c| cont_listen(g, c, false, Listener::ActivatorElectrode)),
    ("DetectorBehaviour.OnDestroy", |g, c| cont_listen(g, c, false, Listener::Detector)),
    // gapA r8 hand entries. TyreDeflationBehaviour.Unstabbed(stabbing): PopTyre(stabbing.point) (the probe's `Stab(.. point=)`).
    ("TyreDeflationBehaviour.Unstabbed", |g, c| { c.with(2)?; let t = arena(&g.tyres, c.sid, |x| x.id, "tyres")?; g.tyre_pop(t, Some(c.rec("Stab(", "point")?)); VOID }),
    // PhysicalBehaviour.get_ShowOutline: the showOutline field.
    ("PhysicalBehaviour.get_ShowOutline", |g, c| { let a0 = c.with(1)?.p_usize(g, 0)?; Ok(Enc::enc(g.physicals[a0].showOutline)) }),
    // SliderTool.OnSelect/OnHold: DistanceWireTool's with the Slider kind (blood_wire.rs wire_on_select / wire_on_hold).
    ("SliderTool.OnSelect", |g, c| { c.with(0)?; g.wire_on_select(); VOID }),
    ("SliderTool.OnHold", |g, c| { c.with(0)?; g.wire_on_hold(); VOID }),
    ("LinkDeviceTool.GetEndObject", |g, c| { c.with(0)?; Ok(Enc::enc(g.link_end_body())) }),
    ("SerpentineBeltTool.GetEndObject", |g, c| { c.with(0)?; Ok(Enc::enc(g.link_end_body())) }),
    // gapA r8: the other events' (un)registering Awakes/Starts/OnDestroys (gen_listen, MapConfigProbe.GenListeners).
    ("ActivationToggleBehaviour.Awake", |g, c| { c.with(1)?; let t = arena(&g.activation_toggles, c.sid, |x| x.id, "activation_toggles")?; gen_listen(g, c, true, t, &[Ev::OnSingleUse], Some(Listener::ActivationToggle(t)), Game::activation_toggle_awake) }),
    ("ActivationToggleBehaviour.Start", |g, c| { c.with(1)?; let t = arena(&g.activation_toggles, c.sid, |x| x.id, "activation_toggles")?; gen_listen(g, c, true, t, &[Ev::OnSingleUse], Some(Listener::ActivationToggle(t)), Game::activation_toggle_start) }),
    ("ActivationToggleBehaviour.OnDestroy", |g, c| { c.with(1)?; let t = arena(&g.activation_toggles, c.sid, |x| x.id, "activation_toggles")?; gen_listen(g, c, false, t, &[Ev::OnSingleUse], Some(Listener::ActivationToggle(t)), Game::activation_toggle_on_destroy) }),
    ("ActOnDisintegrate.Awake", |g, c| { c.with(1)?; let i = arena(&g.act_on_disintegrates, c.sid, |x| x.id, "act_on_disintegrates")?; gen_listen(g, c, true, i, &[Ev::OnDisintegration], None, Game::act_on_disintegrate_awake) }),
    ("ActOnDisintegrate.OnDestroy", |g, c| { c.with(1)?; let i = arena(&g.act_on_disintegrates, c.sid, |x| x.id, "act_on_disintegrates")?; gen_listen(g, c, false, i, &[Ev::OnDisintegration], None, Game::act_on_disintegrate_on_destroy) }),
    ("ExistInThermalVision.OnDestroy", |g, c| { c.with(1)?; let id = c.sid; let pre = g.listen_count(id, Ev::OnThermalVisionToggle); g.exist_in_thermal_vision_on_destroy(id); Ok(format!("lis={pre},{}", g.listen_count(id, Ev::OnThermalVisionToggle))) }),
    ("JavelinBehaviour.OnDestroy", |g, c| { c.with(1)?; let j = arena(&g.javelins, c.sid, |x| x.id, "javelins")?; gen_listen(g, c, false, j, &[Ev::OnLaunch], None, Game::javelin_on_destroy) }),
    ("InvisibleOptionInContextMenu.OnDestroy", |g, c| { c.with(1)?; let i = arena(&g.invisibles, c.sid, |x| x.id, "invisibles")?; gen_listen(g, c, false, i, &[Ev::LimbStatusToggled], None, Game::invisible_on_destroy) }),
    ("IonBoltBehaviour.Awake", |g, c| { c.with(1)?; let b = arena(&g.ion_bolts, c.sid, |x| x.id, "ion_bolts")?; gen_listen(g, c, true, b, &[Ev::OnEndOfLife], None, Game::ion_bolt_awake) }),
    ("DragTool.OnDestroy", |g, c| { c.with(1)?; gen_listen(g, c, false, 0, &[], Some(Listener::Drag), |g, _| g.drag_tool_on_destroy()) }),
    ("LiquidOutletBehaviour.OnDestroy", |g, c| { let l = c.with(1)?.p_usize(g, 0)?; let pre = g.liquid_outlet_listeners(l); g.liquid_outlet_on_destroy(l); Ok(format!("lis={pre},{}", g.liquid_outlet_listeners(l))) }),
    // Global.CameraPosition: the main camera's localPosition (the PPG_CAMREC GlobalCamera record, dump/fnmaped.rs).
    ("Global.get_CameraPosition", |g, c| { c.with(0)?; Ok(Enc::enc(g.camera_control.position)) }),
    ("Global.set_CameraPosition", |g, c| { let v = c.with(1)?.p_arr::<3>(0)?; g.camera_control.position = v; VOID }),
    // fnmaped r16: MaterialAwareRaycast / GetLaserEndPoint on MapConfigProbe.LaserCase's rays (the LaserHit words: laser_words).
    ("Utils.MaterialAwareRaycast", |g, c| {
        let (o, d) = (c.v::<2>(0), c.v::<2>(1));
        let h = g.material_aware_raycast(o, d, c.f(2), c.n::<i32>(3)? as u32, c.n(4)?, c.n(5)?, c.f(6));
        Ok(format!("laser:{}", laser_words(g, &h)))
    }),
    ("Utils.GetLaserEndPoint", |g, c| {
        let (o, d) = (c.v::<2>(0), c.v::<2>(1));
        let mut list = laser_list(c.args[2])?;
        g.get_laser_end_point(o, d, &mut list, c.n::<i32>(3)? as u32, c.f(4), c.n(5)?);
        let after = list.iter().map(|h| laser_words(g, h)).collect::<Vec<_>>().join("/");
        Ok(format!(" out={};{};lasers:{after};{};{};{}", c.args[0], c.args[1], c.args[3], c.args[4], c.args[5]))
    }),
    // LiquidContainerLeakingBehaviour.Unstabbed(stabbing): the victim's SendMessage (send_unstabbed, which holds the
    // StabCausesWound check). The probe's Stabbing: `Stab(stabber=<id> wound=<0/1> normal=x,y point=x,y)`; the stabber's
    // StabCausesWound is the recorded one (the dump has no such field; the probe turns it off every other case).
    ("LiquidContainerLeakingBehaviour.Unstabbed", |g, c| {
        let l = g.liquid_leaks.iter().position(|x| x.id == c.id).ok_or("no liquid_leaks entry")?;
        let stabber = phys_by_id(g, c.rec::<i32>("Stab(", "stabber")?)?;
        g.physicals[stabber].StabCausesWound = c.rec::<bool>("Stab(", "wound")?;
        let s = super::super::stab::Stabbing { stabber, victim: g.liquid_leaks[l].physical, normal: c.rec("Stab(", "normal")?, point: c.rec("Stab(", "point")? };
        g.send_unstabbed(s);
        // ListenPost puts the probe's StabCausesWound back before the PO snapshot.
        g.physicals[stabber].StabCausesWound = true;
        VOID
    }),
    ("Utils.ClosestPointFix", |g, c| { use super::super::shatter::ShatterHost; let c = c.with(2)?; let p = c.p_usize(g, 0)?; let v = c.p_arr::<2>(1)?; Ok(Enc::enc(g.closest_point_fix(p, v))) }),
    ("Utils.GetMinHeatTransferSpeed", |g, c| {
        let (a, b) = (c.arg_phys(g, 0)?, c.arg_phys(g, 1)?);
        Ok(hex(g.min_heat_transfer_speed(a, b)))
    }),
    // fnmaped r18: CalculateBreakForceForCable(joint, base, referenceMass, massInfluence) on MapConfigProbe.BreakCase's joints:
    // one with a fixed joint the loader adopts through the game path; else the joint's record (a limb's HingeJoint2D, the Sea
    // panel's MapHingeJoint2D/MapFixedJoint2D with no connectedBody) on its own body's mass (its `own=` Rigidbody2D, or the
    // PhysicalBehaviour on its Clone row) and its connected one's.
    ("Utils.CalculateBreakForceForCable", |g, c| {
        let id = reference(c.args[0]).ok_or("joint arg is no reference")?;
        let (base, rm, mi) = (c.f(1), c.f(2), c.f(3));
        if let Some(&f) = g.adopted_joints.get(&id) {
            return Ok(hex(g.calculate_break_force_for_cable(f, base, rm, mi)));
        }
        let r = c.s.records.iter().find(|r| r.id == id && r.ty.ends_with("Joint2D")).ok_or_else(|| format!("joint #{id}: not adopted, no joint record"))?;
        // The PhysicalBehaviour on the Clone row holding component `key`.
        let phys_of = |key: String| -> Result<usize, String> {
            let comps = c.s.of("Clone").filter_map(|r| r.get("comps")).find(|x| x.trim_matches(['[', ']']).split(';').any(|e| e == key)).ok_or_else(|| format!("no Clone row with {key}"))?;
            let pid = comps.trim_matches(['[', ']']).split(';').find_map(|e| e.strip_prefix("PhysicalBehaviour:")?.parse::<i32>().ok()).ok_or_else(|| format!("{key}: no PhysicalBehaviour"))?;
            phys_by_id(g, pid)
        };
        let mass = |p: usize| g.physicals[p].rigidbody.mass(&g.world);
        let attached = match r.get("own").and_then(reference) {
            Some(rb) => phys_of(format!("Rigidbody2D:{rb}"))?,
            None => phys_of(format!("HingeJoint2D:{id}"))?,
        };
        let connected = match r.get("connected").and_then(reference) {
            Some(rb) => Some(mass(c.record_index(g, rb).map(|x| x.1).or_else(|_| phys_of(format!("Rigidbody2D:{rb}")))?)),
            None => None,
        };
        Ok(hex(super::super::blood_wire::cable_break_force(base, mass(attached), connected, rm, mi)))
    }),
    // fnmaped r18: GetHierarchyPath(transform) on the Transform tree of the Clone rows (save.rs hierarchy_path_of, the twin
    // the save path uses): the transform's row, its root's rows from the nearest index-0 row before it.
    ("Utils.GetHierarchyPath", |_g, c| {
        let key = format!("Transform:{}", reference(c.args[0]).ok_or("transform arg is no reference")?);
        let rows: Vec<_> = c.s.of("Clone").collect();
        let at = rows.iter().position(|r| r.get("comps").is_some_and(|x| x.trim_matches(['[', ']']).split(';').any(|e| e == key))).ok_or("no Clone row with the Transform")?;
        let start = (0..=at).rev().find(|&i| rows[i].get("index") == Some("0")).ok_or("no root row")?;
        let idx = |i: usize| rows[i].get("index").and_then(|v| v.parse::<usize>().ok()).unwrap_or(0);
        let parent = |x: usize| rows[start + x].get("parent").and_then(|v| v.parse::<usize>().ok());
        let name = |x: usize| rows[start + x].name.as_str();
        Ok(format!("\"{}\"", super::super::save::hierarchy_path_of(idx(at), &name, &parent)))
    }),
    // fnmaped r19: Global.ProcessSnappingShortcuts with MapConfigProbe's seeded inputs, FnCase `snap=<mask>,<GridSnap hex>,<AngleSnap>`:
    // InputSystem.Down true for the mask's actions (incrementGrid, decrementGrid, incrementAngleSnap, decrementAngleSnap) only.
    // ret as the probe writes it: the preferences after and the notifications the call showed.
    ("Global.ProcessSnappingShortcuts", |g, c| {
        let snap = c.s.of("FnCase").next().and_then(|r| r.get("snap")).ok_or("no snap= (MapConfigProbe.Mark)")?;
        let p: Vec<&str> = snap.split(',').collect();
        let mask: u32 = p[0].parse().map_err(|_| "snap mask")?;
        (g.prefs.GridSnap, g.prefs.AngleSnap) = (f(p[1]), p[2].parse().map_err(|_| "snap angle")?);
        for (i, id) in ["incrementGrid", "decrementGrid", "incrementAngleSnap", "decrementAngleSnap"].into_iter().enumerate() {
            g.input.controls.insert(id.to_string(), super::super::tool::Control { down: mask >> i & 1 == 1, held: false, up: false });
        }
        let before = g.notifications.len();
        g.process_snapping_shortcuts();
        let notes: Vec<String> = g.notifications[before..].iter().map(|n| n.replace(' ', "_")).collect();
        Ok(format!("grid={},angle={},notes={}", hex(g.prefs.GridSnap), g.prefs.AngleSnap, notes.join("|")))
    }),
    // fnmaped r19: EnumerateChildrenAndSelf(transform) on the loaded clone's Transform tree (tool.rs enumerate_children_and_self,
    // which FlipSelection and SaveOutline walk too): the Transforms as MapConfigProbe.Fmt writes them (`@name#id`, names from
    // the Clone rows; a RectTransform is one too).
    // fnmaped r20: MapConfig.PrepareWeatherInstance(prefab) on the linked map's MapConfig (map.rs map_config_weather_clone):
    // the arg's Snow/Rain/Fog prefab by name; ret the new clone's GameObject (its Transform's id + 2) as the recorder writes it.
    ("MapConfig.PrepareWeatherInstance", |g, c| {
        let m = g.map_config.clone().ok_or("no MapConfig in the rewrite")?;
        let name = c.args[0].strip_prefix('@').and_then(|a| a.split('#').next()).ok_or("prefab arg is no reference")?;
        let w = m.weather.iter().flatten().find(|w| w.objects[0].name.replace(' ', "_") == name).ok_or_else(|| format!("{name}: no weather prefab of the MapConfig"))?.clone();
        let inst = g.map_config_weather_clone(&m, &w);
        Ok(format!("@{name}(Clone)#{}", inst.objects[0].components[0] + 2))
    }),
    // batch4 r12: NextFixedUpdateCoroutine(action), the action a wire's OnFlip lambda (`flip:<wire id>`, FnTiny.Fmt): the first
    // MoveNext yields WaitForFixedUpdate; drained (PPG_FNPROBE_DRAIN, drain=1) the action runs, then the end.
    ("Utils.NextFixedUpdateCoroutine", |g, c| {
        let a = c.args.first().ok_or("no arg")?;
        let id: i32 = a.strip_prefix("flip:").ok_or(format!("arg {a}: no flip:<wire id>"))?.parse().map_err(|_| format!("arg {a}"))?;
        let w = g.blood_wires.iter().position(|w| w.id == id).ok_or(format!("no wire #{id}"))?;
        if c.field("drain") != Some("1") { return Ok("step:WaitForFixedUpdate".into()) }
        g.blood_wire_flip_reconfigure(w);
        Ok("steps:WaitForFixedUpdate,end".into())
    }),
    // batch4 r12: the listener list from FnTiny.ToolWheelListeners's record; the handler arg is `listener:<owner id>`.
    ("ToolWheelBehaviour.add_OnToolVisibilityChanged", |g, c| { let o = listener_arg(g, c.args.first().ok_or("no arg")?)?; g.tool_wheel_add_on_tool_visibility_changed(o); VOID }),
    ("ToolWheelBehaviour.remove_OnToolVisibilityChanged", |g, c| { let o = listener_arg(g, c.args.first().ok_or("no arg")?)?; g.tool_wheel_remove_on_tool_visibility_changed(o); VOID }),
    // batch4 r11: the undo history from FnTiny.UndoHistory's record; the action args are `undo:<index>` (FnTiny.Fmt).
    ("UndoControllerBehaviour.DeregisterAction", |g, c| { let i = undo_arg(c.args.first().ok_or("no arg")?)?; g.undo_deregister_action(i); VOID }),
    // FindRelevantAction(o, out result): the bool, out= o and the action found.
    ("UndoControllerBehaviour.FindRelevantAction", |g, c| {
        if c.args.len() != 2 { return Err(format!("arity: 2 args, the game passed {}", c.args.len())) }
        let i = g.undo_find_relevant_action(undo_object(c, c.args[0])?);
        Ok(format!("{} out={};{}", bit(i.is_some()), c.args[0], i.map_or("null".to_string(), |i| format!("undo:{i}"))))
    }),
    ("Utils.EnumerateChildrenAndSelf", |g, c| {
        let id = reference(c.args[0]).ok_or("transform arg is no reference")?;
        // A selection Outline (no clone object: the PhysicalBehaviour's id) has no children.
        let ts = if g.physicals.iter().any(|p| !p.destroyed && p.ids.selectionOutlineObject != 0 && p.ids.selectionOutlineObject - 2 == id) {
            vec![id]
        } else if g.retained.iter().chain(&g.unsynced).any(|i| i.objects.iter().any(|x| x.components.contains(&id))) {
            g.with_component_instance(id, |g, inst, o| g.enumerate_children_and_self(inst, o))
        } else if let Some(ts) = g.runtime_subtree(id) {
            ts
        } else {
            return Err(format!("Transform #{id}: on no loaded clone"));
        };
        let name = |t: i32| {
            let key = format!("Transform:{t}");
            c.s.of("Clone").find(|r| r.get("comps").is_some_and(|x| x.trim_matches(['[', ']']).split(';').any(|e| e.ends_with(&key)))).map_or("?".to_string(), |r| r.name.clone())
        };
        Ok(format!("[{}]", ts.iter().map(|&t| format!("@{}#{t}", name(t))).collect::<Vec<_>>().join(";")))
    }),
];

/// Methods that read no object state: the return value from the args alone.
fn pure(name: &str, args: &[&str]) -> Option<String> {
    let class = name.strip_suffix(".GetLiquidID");
    if let Some(class) = class.filter(|c| super::super::syringe::is_syringe(c)) {
        return Some(super::super::syringe::liquid_id(class).map_or("null".into(), |id| format!("\"{}\"", id.replace(' ', "_"))));
    }
    use super::super::{blood_wire, ceiling_turret, fire, mathf, physical, utils};
    let fa = |i: usize| f(args[i]);
    let va = |i: usize| floats::<2>(args[i]);
    let text = |i: usize| args[i].trim_matches('"').to_string();
    Some(match name {
        // ComputeConvexHull(points, hull): the hull's count; `hull` (the recorder writes the args after the call) must hold the
        // rewrite's hull. The recorded points are already sorted by the game's Array.Sort.
        "Utils.ComputeConvexHull" => {
            let list = |a: &str| -> Vec<[f32; 2]> { split_args(a.trim_start_matches('[').trim_end_matches(']')).into_iter().filter_map(|s| item_arr::<2>(s).ok()).collect() };
            let hull = super::super::serpentine_belt::convex_hull(&mut list(args[0]));
            let ours = Enc::enc(hull.clone());
            if ours == args[1] { hull.len().to_string() } else { format!("hull {ours}") }
        }
        "Utils.NaNFallback" => hex(super::super::physical::nan_fallback(f(args[0]), f(args[1]))),
        "Utils.StandariseAngle" => hex(utils::standarise_angle(fa(0))),
        "Utils.NormaliseAngle" => hex(utils::normalise_angle(fa(0))),
        "Utils.CelsiusToFahrenheit" => hex(super::super::celsius_to_fahrenheit(fa(0))),
        "Utils.CelsiusToKelvin" => hex(super::super::celsius_to_kelvin(fa(0))),
        "Utils.FahrenheitToCelsius" => hex(super::super::fahrenheit_to_celsius(fa(0))),
        "Utils.KelvinToCelsius" => hex(super::super::kelvin_to_celsius(fa(0))),
        "Utils.Triangle" => hex(ceiling_turret::triangle(fa(0))),
        "Utils.Mod" => hex(physical::utils_mod(fa(0), fa(1))),
        "Utils.Cosh" => hex(blood_wire::cosh(fa(0))),
        "Utils.GetLerpFactorDeltaTime" => hex(mathf::lerp_factor_dt(fa(0), fa(1))),
        "Utils.SmoothApproach" => hex(fire::smooth_approach(fa(0), fa(1), fa(2), fa(3))),
        "Utils.DistanceFromPointToLineSegment" => hex(blood_wire::distance_to_line_segment(va(0), va(1), va(2))),
        "Utils.SqrdDistanceFromPointToLineSegment" => hex(blood_wire::sqrd_distance_to_segment(va(0), va(1), va(2))),
        "Utils.ChangeRedToOrange" => vec(super::super::circulation::change_red_to_orange(floats(args[0]))),
        "Utils.Rotate" => vec(super::super::value_types::utils_rotate(va(0), fa(1), va(2))),
        "Utils.HasLayer" => bit(physical::has_layer(args[0].parse::<i32>().ok()? as u32, args[1].parse().ok()?)),
        "Utils.GetTemperatureUnitSuffix" => quoted(utils::get_temperature_unit_suffix(args[0].parse().ok()?)),
        "Utils.EscapeRichText" => quoted(&utils::escape_rich_text(&text(0))),
        "Utils.GetFormattedByteString" => quoted(&utils::get_formatted_byte_string(args[0].parse().ok()?)),
        "Utils.GetMD5AsString" => quoted(&crate::ui::data::md5_string(&text(0))),
        "Utils.MapRange" => hex(mathf::map_range(fa(0), fa(1), fa(2), fa(3), fa(4))),
        "Utils.GetPerlin2Mapped" => vec3(mathf::perlin2_mapped(fa(0), fa(1))),
        _ => return None,
    })
}

#[derive(Default)]
struct Tally {
    pass: usize,
    fail: usize,
    /// gapB r2: cases whose recording is older than a field they need (Case::field_since).
    old: usize,
    first: Option<String>,
}

/// The map's record types: the per-case snapshots leave them out (the recorder's Dump.NoMap) and the "PM" snapshot
/// before the cases holds them once. The first three come before everything in a full snapshot, the rest after.
const MAP_FRONT: &[&str] = &["PhysicalProperties", "PhysicalBehaviour", "MapLightBehaviour", "PlayAudioAtRandom"];
/// gapC r8: FnTiny's ParticleSystem field list `<field>:<bits>;...`.
fn ps_fields(v: &str) -> Vec<(String, String)> {
    v.split(';').filter_map(|x| x.split_once(':')).map(|(f, b)| (f.to_string(), b.to_string())).collect()
}

/// gapC r11: FnTiny.MpbOf's `<name>:<value>;...` (a SpriteRenderer's MaterialPropertyBlock) as a map; None for "?" (no shader
/// seen). "-" (an empty block) is an empty map. Values through mpb_norm.
fn mpb_map(v: &str) -> Option<std::collections::BTreeMap<String, String>> {
    match v {
        "?" => None,
        "-" => Some(Default::default()),
        // checker limit: _MainTex skipped: a SpriteRenderer's GetPropertyBlock carries its sprite texture, which ours draws apart.
        v => Some(ps_fields(v).into_iter().filter(|(n, _)| n != "_MainTex").map(|(n, x)| (n, mpb_norm(&x))).collect()),
    }
}

/// gapC r11: a block value as both sides show it: all zero bits as unset (2020.3's GetFloat/GetVector of an unset name), a
/// texture only as set or not.
// checker limit: a texture by presence only (ours keeps the ObjRef, not the name); add Texture2D names when a set texture differs.
fn mpb_norm(x: &str) -> String {
    if let Some(t) = x.strip_prefix('t') {
        return if t == "null" { String::new() } else { "t".into() };
    }
    if x.get(1..).is_some_and(|h| h.split(',').all(|h| h == "00000000")) { String::new() } else { x.to_string() }
}

/// gapC r11: ours' block of SpriteRenderer `id` as mpb_map shows it (a Color linear, as MaterialPropertyBlock.SetColor
/// stores it in the linear project).
fn ours_mpb(g: &Game, id: i32) -> Option<std::collections::BTreeMap<String, String>> {
    use crate::game::props::Prop;
    let at = g.item_sprite(id)?;
    let h = |v: &[f32]| v.iter().map(|x| format!("{:08x}", x.to_bits())).collect::<Vec<_>>().join(",");
    Some(g.get_property_block(at).0.iter().filter_map(|(n, p)| {
        let x = match p {
            Prop::Float(v) => format!("f{}", h(&[*v])),
            Prop::Vector(v) => format!("v{}", h(v)),
            Prop::Color(c) => format!("v{}", h(&crate::custom_material::linear(*c).to_array())),
            Prop::Texture(_) => "t".into(),
            Prop::VectorArray(_) => return None,
        };
        Some((n.clone(), mpb_norm(&x)))
    }).filter(|(n, x)| !x.is_empty() && n != "_MainTex").collect())
}

/// gapC r11: ours' sortingLayerID,sortingOrder of SpriteRenderer `id` (`ItemSprite order=`).
fn ours_order(g: &Game, id: i32) -> Option<String> {
    use crate::game::visual::SpriteAt;
    let (l, o) = match g.item_sprite(id)? {
        SpriteAt::Physical(p) => (g.physicals[p].sprite_layer, g.physicals[p].sprite?.2),
        SpriteAt::Renderer(r) => (g.renderers[r].layer, g.renderers[r].sprite.2),
    };
    Some(format!("{l},{o}"))
}

/// gapC r9: ours' storage of a C# static field `<Class>.<field>` (FnTiny.StaticsOf), for the ones the rewrite keeps.
// checker limit: one line per static; add one when a recording's sf= names a field ours keeps elsewhere.
fn static_field<'a>(g: &'a mut Game, name: &str) -> Option<&'a mut dyn crate::game::Field> {
    Some(match name {
        "PhysicalBehaviour.AmbientTemperature" => &mut g.AmbientTemperature,
        "WoodStrutBehaviour.lastStressSoundTime" => &mut g.wire_tool.lastStressSoundTime,
        "UndoControllerBehaviour.currentHistoryPosition" => &mut g.undo.currentHistoryPosition,
        "UseWireBehaviour.lastWarningTime" => &mut g.signals.last_warning,
        _ => return None,
    })
}

/// gapC r8: ours' embedded system for component `sid`'s ParticleSystem field `field`, for the scripts that drive one.
/// gapC r18: the bleeding particle of BleedingParticleBehaviour `sid`: its row among the snapshot's records (bleeding_rows' order).
fn bleeding_of(g: &Game, s: &Snapshot, sid: i32) -> Option<usize> {
    let k = s.of("BleedingParticleBehaviour").position(|r| r.id == sid)?;
    super::bleeding_rows(g).get(k).copied()
}

fn ps_field(g: &Game, sid: i32, field: &str) -> Option<usize> {
    use crate::game::EmbeddedDrive as D;
    g.embedded_ps.iter().position(|e| e.owner == sid && match e.drive {
        D::IonThruster => field == "FX",
        D::Magnet => field == "particleSystem",
        D::ParticleMachine { k, .. } => ["SmokeParticles", "FireParticles", "ElectricityParticles"][k] == field,
        D::Muzzle(_) => field == "flash",   // gapC r16: FirearmBehaviour.flash (Awake's MuzzleSmoke clone)
        _ => false,
    }).or_else(|| g.field_ps(sid, field))   // gapC r10: any other script's field (particles.rs field_ps)
        // v52h round 2: InjectableBehaviour.ImbueParticles (Start's GetComponentInChildren).
        .or_else(|| g.v45.injectables.iter().find(|x| x.id == sid && field == "ImbueParticles").and_then(|x| x.ImbueParticles))
}

/// gapC r20: the container's GlowInTheDarkBehaviour before the call (its PI Clone record's `comps=` entry; none: no glow) as ours'
/// glow on `o` (a seed's AddComponent or an earlier case's OnEnterContainer made it; the calls read only its presence).
fn glow_pre(g: &mut Game, c: &Case, o: Owner) {
    let Some(arg) = c.args.iter().rev().find_map(|a| reference(a)).map(|v| v.to_string()) else { return };
    glow_pre_on(g, c, o, &arg);
}

/// glow_pre for the Clone holding component `arg` (fc41 r10: OnLiquidExit's self).
fn glow_pre_on(g: &mut Game, c: &Case, o: Owner, arg: &str) {
    let Some(comps) = c.s.of("Clone").filter_map(|r| r.get("comps")).find(|v| v.split([';', '[', ']']).any(|e| e.rsplit(':').next() == Some(arg))) else { return };
    let id = comps.split([';', '[', ']']).find_map(|e| e.strip_prefix("GlowInTheDarkBehaviour:")).and_then(|v| v.parse::<i32>().ok());
    g.glows.retain(|x| x.owner != o || Some(x.id) == id);
    if let Some(id) = id && !g.glows.iter().any(|x| x.id == id) {
        g.glows.push(super::super::glow::GlowInTheDark { id, owner: o, light: None, glow_go: 0, light_go: 0 });
    }
}

/// gapC r21: an ExplosiveBehaviour on the container in the PI (its Clone `comps=`) that ours lacks, as an added one with its
/// ItemScript record's Tag, else "nitro" (Nitroglycerine.OnExitContainer reads only the first one's Tag and id).
fn nitro_pre(g: &mut Game, c: &Case, o: Owner) {
    let Some(arg) = c.args.iter().rev().find_map(|a| reference(a)).map(|v| v.to_string()) else { return };
    let Some(comps) = c.s.of("Clone").filter_map(|r| r.get("comps")).find(|v| v.split([';', '[', ']']).any(|e| e.rsplit(':').next() == Some(arg.as_str()))) else { return };
    let p = g.container_physical(o);
    for id in comps.split([';', '[', ']']).filter_map(|e| e.strip_prefix("ExplosiveBehaviour:")).filter_map(|v| v.parse::<i32>().ok()) {
        // No ItemScript record (a limb's container: the recorder writes item scripts only): Nitroglycerine.OnEnterContainer is the
        // game's only AddComponent<ExplosiveBehaviour>, and it sets Tag "nitro".
        let tag = c.s.of("ItemScript").find(|r| r.id == id).and_then(|r| r.get("Tag")).unwrap_or(crate::il!(str "Nitroglycerine::OnEnterContainer" 0));
        if g.explosives.iter().any(|x| x.id == id) {
            continue;
        }
        let index = 1000 + g.explosives.iter().filter(|x| x.added && x.physical == Some(p)).count();
        g.explosives.push(crate::game::explosion::Explosive { id, physical: Some(p), index, added: true, Tag: tag.to_string(), ..Default::default() });
    }
}

/// gapC r19: the recorder's `IgnoredCollisions pairs=[a,b;...]` (Physics2D.GetIgnoreCollision of every collider pair).
fn ignored_pairs(s: &Snapshot) -> Option<std::collections::HashSet<(i32, i32)>> {
    let r = s.of("IgnoredCollisions").next()?;
    Some(r.get("pairs")?.trim_matches(['[', ']']).split(';').filter_map(|p| {
        let (a, b) = p.split_once(',')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    }).collect())
}

/// gapC r19: `s` has the map's Clone records and none of them is one of the PM's: the map was loaded again since.
fn reloaded(pm: &Snapshot, s: &Snapshot) -> bool {
    let map = |x: &Snapshot| x.of("Clone").filter(|r| r.get("root") == Some("Map")).map(|r| r.id).collect::<std::collections::HashSet<i32>>();
    let now = map(s);
    !now.is_empty() && now.is_disjoint(&map(pm))
}

const MAP_BACK: &[&str] = &["Clone", "MapHingeJoint2D", "MapFixedJoint2D", "MapSpringJoint2D", "MapDistanceJoint2D", "TransformChain"];

/// A per-case snapshot with the PM's map records put back where a full snapshot has them (each type's map records
/// after or before the human's as the recorder writes them; records the case already has stay as they are).
fn with_map(s: &Snapshot, pm: Option<&Snapshot>) -> Snapshot {
    let Some(pm) = pm else { return s.clone() };
    let have: std::collections::HashSet<(&str, i32)> = s.records.iter().map(|r| (r.ty.as_str(), r.id)).collect();
    // A human's limbs in the PM are the human's at that time: after a respawn they are gone (the case has the new one's).
    // v52h r12: and the human's Clone records (a bleed's BleedingParticle(Clone) since destroyed kept its psm slot: "psm 0 listed twice").
    let human = |r: &&super::Record| (r.ty == "PhysicalBehaviour" && r.get("Properties").is_some_and(|v| v.starts_with("@Human#"))) || (r.ty == "Clone" && r.get("root") == Some("Human"));
    // A case with map lights of its own has the live map's: the PM's are an earlier map's (MapLoaderBehaviour.Awake reloads it).
    let own_lights = s.of("MapLightBehaviour").next().is_some();
    let pick = |types: &[&str]| pm.records.iter().filter(|r| types.contains(&r.ty.as_str()) && !have.contains(&(r.ty.as_str(), r.id)) && !human(r) && !(own_lights && r.ty == "MapLightBehaviour")).cloned().collect::<Vec<_>>();
    let mut records = pick(MAP_FRONT);
    records.extend(s.records.iter().cloned());
    // A map joint no body's edge list names any more (broken since the PM: a Window, a table leg) is gone.
    let alive: std::collections::HashSet<&str> = s.of("b2Edges").filter_map(|r| r.get("joints")).flat_map(|j| j.split(';')).collect();
    let live = |r: &super::Record| !r.ty.starts_with("Map") || r.get("joint") == Some("null") || alive.contains(format!("{}#{}", r.name, r.id).as_str());
    // A case with the map's Clone records has them all: a PM one it lacks is an object destroyed since (a shattered Window).
    let own_clone = s.of("Clone").any(|r| r.get("root") == Some("Map"));
    // A broken map joint (a wire snapped since the PM) was destroyed, and with it the wire on its GameObject
    // (WireBehaviour.OnJointBreak2D: Destroy(this)): out of the PM Clone's component list, so the loader destroys both.
    let broken: std::collections::HashSet<String> = pick(MAP_BACK).iter().filter(|r| r.ty.starts_with("Map") && r.ty.ends_with("Joint2D") && !live(r)).map(|r| r.id.to_string()).collect();
    let mut back = pick(MAP_BACK);
    for r in back.iter_mut().filter(|r| r.ty == "Clone" && r.get("root") == Some("Map")) {
        let Some(comps) = r.get("comps") else { continue };
        let list = super::list(comps);
        if !list.iter().any(|c| c.rsplit_once(':').is_some_and(|x| broken.contains(x.1))) {
            continue;
        }
        let kept: Vec<&str> = list.iter().copied().filter(|c| !c.rsplit_once(':').is_some_and(|x| broken.contains(x.1) || x.0.ends_with("WireBehaviour"))).collect();
        let v = format!("[{}]", kept.join(";"));
        r.fields.iter_mut().find(|(k, _)| k == "comps").unwrap().1 = v;
    }
    records.extend(back.into_iter().filter(|r| live(r) && !(own_clone && r.ty == "Clone" && r.get("root") == Some("Map"))));
    Snapshot { pt: s.pt.clone(), snap: s.snap.clone(), records }
}

/// `PPG_FNCHECK_SHARD=k/n`: only the classes whose name hash is k mod n (tools/fncheck_par.sh runs n of them at once).
/// `PPG_FNCHECK_SKIP=<fncheck.txt>`: functions with a PASS row there are not re-checked (fncheck_par.sh --skip-pass).
/// gapC r3 mutation sweep (tools/fngapc/mutsweep.sh): `PPG_MUTATE=all` or `PPG_MUTATE=<Class.Method>,...` skips the rewrite
/// call of those cases (ours is VOID "", the state stays the PI's). A function that still passes compares nothing on that file.
/// gapA r18: a pure row's result had the call been skipped (the PI mutant): the return value's default (a number or float
/// bits 0, False, null) and, on a seeded row (`<ret> set(..) after=..`), no field set and the args as passed.
fn skipped_call(ours: &str, args: &[&str]) -> String {
    let Some((ret, _)) = ours.split_once(" set(") else { return default_value(ours) };
    format!("{} set() after={}", default_value(ret), args.get(1..).unwrap_or(&[]).join(";"))
}
/// Each value word of a recorded value at its type's default; keys (`k=`), "void" and the punctuation stay.
fn default_value(v: &str) -> String {
    let word = |w: &str| -> String {
        let hex8 = w.len() == 8 && w.bytes().all(|b| b.is_ascii_hexdigit());
        if hex8 { "00000000".into() } else if w.parse::<i64>().is_ok() { "0".into() } else if w == "True" || w == "False" { "False".into() } else if w.starts_with('"') || w.starts_with('@') || w.starts_with("b64:") || w.starts_with("exc:") { "null".into() } else if bare(w) { "null".into() } else { w.into() }
    };
    // gapA r20: a bare identifier result (a Liquid's identity `BLOOD`, an object's type name `Sprite`) is a reference the
    // skipped call leaves null; `void`, `throw` and a constructor row's `ok` are not values, and a record's type name (before its `(`) stays.
    fn bare(w: &str) -> bool {
        !w.is_empty() && w != "void" && w != "throw" && w != "ok" && w.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') && w.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
    }
    let (mut out, mut w) = (String::new(), String::new());
    for ch in v.chars() {
        // A quoted string's own punctuation stays in the word; so does a b64 word's '=' padding (gapA r19).
        let open_quote = (w.starts_with('"') && !(w.len() > 1 && w.ends_with('"'))) || (w.starts_with("b64:") && ch == '=');
        if ",;()[] =".contains(ch) && !open_quote {
            out += &if ch == '=' || ch == '(' { std::mem::take(&mut w) } else { word(&std::mem::take(&mut w)) };
            out.push(ch);
        } else {
            w.push(ch);
        }
    }
    out + &word(&w)
}

#[test]
fn skipped_call_defaults() {
    assert_eq!(skipped_call("void set(ReadTemperature=c19c5c71 WriteTemperature=c19c5c71) after=c19c5c71", &["Cell(x=1)", "c19c5c71"]), "void set() after=c19c5c71");
    assert_eq!(skipped_call("3f800000,40000000", &[]), "00000000,00000000");
    assert_eq!(skipped_call("True", &[]), "False");
    assert_eq!(skipped_call("\"a,b\" 12 Stat(Name=7)", &[]), "null 0 Stat(Name=0)");
    assert_eq!(skipped_call("[b64:IA==;b64:TW9kcw==] exc:NullReferenceException", &[]), "[null;null] null");
    assert_eq!(skipped_call("BLOOD set()", &[]), "null set() after=");
    assert_eq!(skipped_call("Sprite", &[]), "null");
}

fn mutated(name: &str) -> bool {
    static M: std::sync::OnceLock<Option<Vec<String>>> = std::sync::OnceLock::new();
    M.get_or_init(|| std::env::var("PPG_MUTATE").ok().map(|v| v.split(',').map(String::from).collect())).as_ref().is_some_and(|m| m.iter().any(|x| x == "all" || x == name))
}

/// gapB r15: each reference cache the case reset (Game::ref_reset) against the PO: null in both, or set in both.
/// fc41 r20: not when the game's call threw and left the field at its PI value (it threw before filling it; the reset
/// is only the checker's, and the return compare covers the throw).
fn ref_reset_diffs(g: &mut Game, s: &Snapshot) -> Vec<String> {
    let reset = std::mem::take(&mut g.ref_reset);
    let mut out = Vec::new();
    let threw = s.of("FnResult").next().and_then(|r| r.get("ret")).is_some_and(|v| v.contains("exc:"));
    crate::game::REF_LIVE.with(|l| l.set(true));
    for (class, id, name, pre) in &reset {
        let Some(theirs) = super::clone::script_records(s).find(|(_, r)| r.id == *id).and_then(|(_, r)| r.get(name)) else { continue };
        if threw && theirs == pre {
            continue;
        }
        let Some((_, links)) = super::fnitems::item_fields(g, class, *id) else { continue };
        let Some(ours) = links.into_iter().find(|(n, _)| n == name).map(|(_, f)| f.show()) else { continue };
        if (theirs == "null") != (ours == "null") {
            out.push(format!("{class} #{id} {name} (reset before the call): game {theirs} ours {ours}"));
        }
    }
    crate::game::REF_LIVE.with(|l| l.set(false));
    out
}

fn in_shard(name: &str) -> bool {
    static SKIP: std::sync::OnceLock<std::collections::HashSet<String>> = std::sync::OnceLock::new();
    let skip = SKIP.get_or_init(|| {
        let text = std::env::var("PPG_FNCHECK_SKIP").ok().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        text.lines().filter_map(|l| l.strip_prefix("PASS ")?.split_whitespace().next().map(String::from)).collect()
    });
    if skip.contains(name) {
        return false;
    }
    // `PPG_FNCHECK_ONLY=<Class.Method>[,...]`: those functions alone (a PPG_RANDOM_TRACE look at one case).
    if std::env::var("PPG_FNCHECK_ONLY").is_ok_and(|only| only.split(',').all(|o| o != name)) {
        return false;
    }
    let Ok(v) = std::env::var("PPG_FNCHECK_SHARD") else { return true };
    let (k, n) = v.split_once('/').and_then(|(k, n)| Some((k.parse::<u64>().ok()?, n.parse::<u64>().ok()?))).expect("PPG_FNCHECK_SHARD=k/n");
    let class = name.rsplit_once('.').map_or(name, |c| c.0);
    class.bytes().fold(5381u64, |h, c| h.wrapping_mul(33) ^ c as u64) % n == k
}

/// (recording, function): a recording made by an older recorder, its cases of that function superseded by a re-record
/// (the newer one named after the arrow). They stay on disk; their cases are not run.
const SUPERSEDED: &[(&str, &str)] = &[
    ("fngc1_gen", "BlasterBehaviour.Start"), // -> fngc3_gen (no mirrored case, no muz= then)
    ("fng6_list11_reactor", "WaterBehaviour.GetSubmergedObjects"), // -> fni17_sea, fng6_list11_sea (no SliderJoint2D records then: "unknown joint Heaviest_object_known_to_man#-18750")
    ("fngc1_gen", "AmbientTemperatureGridBehaviour.EnsureExistence"), // -> fngc3_gen (no AmbientGridCells then)
    ("fngc2_gen", "AOEPowerTool.Awake"), // -> fngc3_gen (no selfty=/teff= then)
    ("fngc2_gen", "BloodWireTool.OnToolChosen"), // -> fngc3_gen (no selfty=/tpre= then)
    ("fngc2_gen", "FixedWireTool.OnToolChosen"), // -> fngc3_gen
    ("fngc3_gen", "AOEPowerTool.Awake"), // -> fngc4_gen (no teff= then)
    ("fngc3_gen", "BlasterBehaviour.Start"), // -> fngc4_gen (no muz= then)
    ("fnb2j_gen", "Hover.CheckMouseInput"), // -> fnb2k_gen (no lineWidth then: k=2's fuzzed WireWidth hid the LineRenderer's widthMultiplier)
    ("fng4_items2", "ep.End"),        // -> fni17_ep (no layer=/decal fields then)
    ("fnba9_default", "Global.Update"), // -> fnba10_default, fnba13_default (22 args then: no ShowLimbStatus, seeded item or Steam, and ShowLimbStatus carried over between cases)
    ("fng5_items2", "ep.End"),        // -> fni17_ep
    ("fng6i_items", "ep.End"),        // -> fni17_ep
    ("fni15_reactor", "NewAtomBombExplosionBehaviour.Start"), // -> fni16_reactor (detached parts listed under two roots)
    ("fng5_scale10", "TankBarrelBehaviour.Use"),             // -> fni18_b2 (no layer= fields then)
    ("fnbb7_gen", "BallisticsEmitter.CalculateHardness"), // -> fnbb9_gen (no em=: earlier setter cases changed the live emitter)
    ("fnbb7_gen", "BallisticsEmitter.CanPlayCracks"), // -> fnbb9_gen (no em=: earlier setter cases changed the live emitter)
    ("fnbb7_gen", "BallisticsEmitter.Emit"), // -> fnbb9_gen (no em=: earlier setter cases changed the live emitter)
    ("fnbb7_gen", "BallisticsEmitter.BallisticIteration"), // -> fnbb9_gen (no em=: earlier setter cases changed the live emitter)
    ("fnbb7_gen", "BlasterboltBehaviour.Impact"), // -> fnbb8_gen (rays that hit nothing)
    ("fnbb8_gen", "BlasterboltBehaviour.Impact"), // -> fnbb11_gen (no pool rot=: a reused BloodPuff's splat ejected along identity)
    ("fnbb9_gen", "BallisticsEmitter.RicochetBullet"), // -> fnbb11_gen (no pool rot=)
    ("fnbb9_gen", "BallisticsEmitter.BallisticIteration"), // -> fnbb11_gen (no pool rot=, fuzzed negative totalDistance)
    ("fnbb10_gen", "BallisticsEmitter.BallisticIteration"), // -> fnbb11_gen (no pool rot=)
    ("fnbb10_gen", "AutomaticSentryController.GetTargetMotorSpeed"), // -> fnbb11_gen (no case acquired a target)
    ("fnbb11_gen", "AutomaticSentryController.GetTargetMotorSpeed"), // -> fnbb12_gen (no case acquired a target)
    ("fnbb11_gen", "TeslaCoilLightningBoltBehaviour.Update"), // -> fnbb12_gen (no smoothDt=)
    ("fnbb11_gen", "EnergySwordMotionBlur.OnEnable"), // -> fnbb12_gen (every blur never awoken: the NRE path only)
    ("fnbb12_gen", "AutomaticSentryController.get_Target"), // -> fnbb13_gen (every case null: no target held)
    // -> fni20_items. The map was reloaded after the recording's only PM (its map ids -135xx; the cases' b2World names the
    // new map's bodies, Root#-736xx): that recorder wrote no PM for the reloaded map, so the cases' map is not in the file.
    ("gen6_00", "LimbBehaviour.SynchroniseDismemberment"),
    ("gen6_00", "LimbBehaviour.Crush"),
    ("gen6_00", "GripBehaviour.OnBeforeSerialise"),
    ("gen6_00", "GripBehaviour.Start"),
    ("fng4_scale9", "LimbBehaviour.SynchroniseDismemberment"),
    ("fng4_scale9", "LimbBehaviour.Crush"),
    ("fng4_scale9", "GripBehaviour.OnBeforeSerialise"),
    ("fng4_scale9", "GripBehaviour.Start"),
    ("gen9_00", "GripBehaviour.OnBeforeSerialise"),
    ("gen9_00", "GripBehaviour.Start"),
    ("gen6_00", "GripBehaviour.CreateJoint"),
    ("gen6_00", "GripBehaviour.Attach"),
    ("fng4_scale9", "GripBehaviour.CreateJoint"),
    ("fng4_scale9", "GripBehaviour.Attach"),
    ("gen9_00", "GripBehaviour.CreateJoint"),
    ("gen9_00", "GripBehaviour.Attach"),
    // -> fni19_items. Once a case has crushed UpperLeg (inactive), JointBreakLimits still lists its hinge #-63234 but the
    // recorder then wrote no HingeJoint2D/b2RevoluteJoint record for it: the hinge's state is not in the file.
    ("fnprobe3e", "LimbBehaviour.Crush"),
    // -> fni19_items: the recorder wrote no mouse= then, so MousePosWithSnap's input is not in the file.
    ("gen5_00", "DragTool.get_MousePosWithSnap"),
    ("gen6_00", "DragTool.get_MousePosWithSnap"),
    ("fnbb8_gen", "BlasterboltBehaviour.Impact"), // -> fnbb11_gen (no pool rot=: a reused BloodPuff's splat ejected along identity)
    ("fnbb9_gen", "BallisticsEmitter.RicochetBullet"), // -> fnbb11_gen (no pool rot=)
    ("fnbb9_gen", "BallisticsEmitter.BallisticIteration"), // -> fnbb11_gen (no pool rot=, fuzzed negative totalDistance)
    ("fnbb10_gen", "BallisticsEmitter.BallisticIteration"), // -> fnbb11_gen (no pool rot=)
    ("fnbb10_gen", "AutomaticSentryController.GetTargetMotorSpeed"), // -> fnbb11_gen (no case acquired a target)
    // -> fnb1r12a_gen/fnb1r12b_gen: fnb1r11a's csid= lists only the limbs' identities, so its k=2 case (Left_wall given one)
    // holds no record of the map object's SerialisableIdentity (the game outlines the map's WORLD root, the file cannot say why).
    ("fnb1r11a_gen", "ContraptionOutlineSerialiser.GenerateOutline"),
    // -> fnb1r3b_gen: fnb1/fnb1r3a have no AmbientGridCells rates (GenRun fuzzes the grid's rates and GridSize).
    ("fnb1_gen", "AmbientTemperatureGridBehaviour.TransferHeat"),
    ("fnb1r3a_gen", "AmbientTemperatureGridBehaviour.TransferHeat"),
    ("fnb1_gen", "PersonBehaviour.GenerateBasePose"), // -> fnb1r3a_gen (fnb1 wrote the RagdollPose return as its Name only)
    ("fngb1_gen", "ExplosionCreator.CreateUnderwaterExplosionEffect"), // -> fngb2_gen (no uw= then: the new UnderwaterExplosion not recorded)
    ("fngb3_gen", "Activations.SendOnce"), // -> fngb4_gen (the pasted toggle's physical not in the per-case dump then)
];

pub fn fn_check(path: &Path, rig: &HumanRig) -> Result<(), String> {
    let stem = path.file_name().and_then(|n| n.to_str()).map_or("", |n| n.split('.').next().unwrap_or(""));
    *OWN_DIR.lock().unwrap() = Some(path.with_file_name(format!("{stem}_own")));
    // A recorder PPG_MAPEDITOR recording: the map editor's own checker.
    if std::fs::File::open(path).ok().and_then(|f| std::io::BufRead::lines(std::io::BufReader::new(f)).next()?.ok()).is_some_and(|l| l.starts_with("mapeditor")) {
        return super::super::map_editor::fncheck::fn_check(path);
    }
    let header = header(path)?;
    let mut tally: std::collections::BTreeMap<String, Tally> = Default::default();
    let mut snaps = snapshots(path)?;
    let mut pending: Option<(Snapshot, Result<(Game, String), String>)> = None;
    // gapC r5: every AudioSource's clip name in ours just before the call (the clip compare's "ours changed it" side).
    let clip_of = |g: &Game, id: i32| g.audio.get(id).clip.as_ref().map_or("null".to_string(), |c| c.clip.name.replace(' ', "_"));
    let mut ours_pre_clips: std::collections::BTreeMap<i32, String> = Default::default();
    // v52h: every AudioSource's volume, minDistance and maxDistance in ours just before the call (`ItemAudioSource vol= mind= maxd=`, PPG_AUDIO_VOL).
    let vol_of = |g: &Game, id: i32| { let a = g.audio.get(id); [hex(a.volume), hex(a.min_distance), hex(a.max_distance)] };
    let mut ours_pre_vols: std::collections::BTreeMap<i32, [String; 3]> = Default::default();
    // gapA r17: ours' coroutine count just before the call (a use wire's routine the call started has a later seq).
    let mut ours_pre_seq = 0;
    // gapC r5: a SpriteRenderer's sprite and a TextMeshPro's text in ours by the recorder's names (`ItemSprite sprite=`,
    // `ItemText text=`), and both before the call.
    let sprite_of = |g: &Game, id: i32| {
        use crate::game::visual::SpriteAt;
        let obj = match g.item_sprite(id)? { SpriteAt::Physical(p) => g.physicals[p].sprite.map(|x| x.0), SpriteAt::Renderer(r) => Some(g.renderers[r].sprite.0) };
        Some(obj.and_then(|o| g.assets.as_ref()?.sprite(o).ok()).map_or("?".to_string(), |x| x.name.replace(' ', "_")))
    };
    let text_of = |g: &Game, id: i32| g.world_texts.iter().position(|t| t.id == id).map(|i| format!("\"{}\"", g.text_get(i).replace(' ', "_").replace('\r', "\\r").replace('\n', "\\n")));
    let mut ours_pre_look: std::collections::BTreeMap<(bool, i32), Option<String>> = Default::default();
    // gapC r11: ours' MaterialPropertyBlock and sorting of each recorded SpriteRenderer before the call.
    let mut ours_pre_mpb: std::collections::BTreeMap<i32, std::collections::BTreeMap<String, String>> = Default::default();
    let mut ours_pre_order: std::collections::BTreeMap<i32, String> = Default::default();
    // gapC r16: the PhysicalBehaviour members the dump compare leaves out (its format has none of them): ReflectsLasers,
    // AbsorbsLasers, collisionBufferCount; ours before the call too.
    // gapC r23: and ContinuousActivationTracker (DragTool.OnContinuousUpdate's UseContinuous), as the recorder formats it.
    const PHYS_LINKS: [&str; 4] = ["ReflectsLasers", "AbsorbsLasers", "collisionBufferCount", "ContinuousActivationTracker"];
    let cat = |p: &crate::game::Physical| format!("{{{}}}", p.ContinuousActivationTracker.iter().enumerate().map(|(i, t)| format!("{i}:DeltaInt(Value={} PreviousValue={} PreviousValueFixed={})", t.Value, t.PreviousValue, t.PreviousValueFixed)).collect::<Vec<_>>().join(";"));
    let phys_links = |g: &Game, id: i32| g.physicals.iter().find(|p| p.id == id && !p.destroyed).map(|p| [bit(p.ReflectsLasers), bit(p.AbsorbsLasers), p.collisionBuffer.len().to_string(), cat(p)]);
    let mut ours_pre_phys: std::collections::BTreeMap<i32, [String; 4]> = Default::default();
    // gapA r18: a PhysicalBehaviour's context menu, as the Identity list of its ContextMenuOptions component's Buttons
    // (`ItemScript class=ContextMenuOptionComponent Buttons=[ContextMenuButton(.. Identity="..")..]`), and ours' menu; ours
    // before the call too. The Starts that add a button (Button, LiquidContainerLeaking, ...) wrote nothing compare() reads.
    let menu_rec = |s: &Snapshot, phys: &super::Record| -> Option<String> {
        let id = super::reference(phys.get("ContextMenuOptions")?)?;
        let b = s.of("ItemScript").find(|r| r.id == id)?.get("Buttons")?;
        Some(b.split("Identity=\"").skip(1).filter_map(|x| x.split('"').next()).collect::<Vec<_>>().join(";"))
    };
    let menu_ours = |g: &Game, id: i32| g.physicals.iter().find(|p| p.id == id && !p.destroyed).map(|p| p.menu.iter().map(|b| b.Identity.replace(' ', "_")).collect::<Vec<_>>().join(";"));
    let mut ours_pre_menu: std::collections::BTreeMap<i32, String> = Default::default();
    // gapC r19: ours' Physics2D.IgnoreCollision pairs before the call.
    let mut ours_pre_ignored: std::collections::HashSet<(i32, i32)> = Default::default();
    // gapC r20: ours' Object.Destroy queues before the call (components, objects, the joint queues' lengths) and its glows' count.
    let mut ours_pre_destroys: (Vec<(f64, i32)>, Vec<(f64, i32)>, usize, usize, usize) = Default::default();
    let mut drag_pre: Vec<(i32, Option<f64>)> = Vec::new();
    let mut gore_pre: Vec<(i32, Option<f64>)> = Vec::new();
    // v52h round 2: each limb's HingeJoint2D (its hinges slot, the record id by the limb's name) and its pending Object.Destroy(Joint).
    let mut limb_hinge_pre: Vec<(usize, i32, Option<f64>)> = Vec::new();
    // gapC r7: the objects of the clone holding the self (`Clone active=<inHierarchy>,<self> lrot=`): ours' activeSelf and, for a
    // body-less non-root object, localRotation, by the recorded GameObject id; and both before the call.
    // gapC r12: localPosition and localScale too (the Clone record's lpos= / lscale=; xyz of the Trs).
    let clone_look = |g: &Game, sid: i32| -> std::collections::BTreeMap<i32, (bool, Option<[f32; 4]>, Option<([f32; 3], [f32; 3])>)> {
        let Some(inst) = g.unsynced.iter().chain(&g.retained).find(|i| i.objects.iter().any(|x| x.components.contains(&sid))) else { return Default::default() };
        inst.objects.iter().enumerate().map(|(o, x)| (x.game_object, (x.self_active, (x.parent.is_some() && !inst.bodies.contains_key(&o)).then_some(x.trs.q), (x.parent.is_some() && !inst.bodies.contains_key(&o)).then_some(([x.trs.t[0], x.trs.t[1], x.trs.t[2]], [x.trs.s[0], x.trs.s[1], x.trs.s[2]]))))).collect()
    };
    let mut ours_pre_clone: std::collections::BTreeMap<i32, (bool, Option<[f32; 4]>, Option<([f32; 3], [f32; 3])>)> = Default::default();
    // gapB r22: every GameObject id ours holds (instances and runtime children), before the call: a GameObject the call made in the
    // game under the self's item (a PO `Clone root=Item` row with no PI row: TeslaCoil.Awake's bolts) must be new in ours too.
    let ours_gos = |g: &Game| -> std::collections::HashSet<i32> {
        let inst = g.unsynced.iter().chain(&g.retained).chain(g.runtime_children.values().flatten().filter_map(|c| c.inst.as_ref()));
        inst.flat_map(|i| i.objects.iter().map(|x| x.game_object)).collect()
    };
    let mut ours_pre_gos: std::collections::HashSet<i32> = Default::default();
    // and ours' next instance id: a GameObject the rewrite keeps no Instance of (AntennaReceiver.Awake's "antenna") only takes its id.
    let mut ours_pre_next = 0;
    let mut pm: Option<Snapshot> = None;
    let ver: u32 = snaps.header.iter().chain(header.iter()).find_map(|l| l.strip_prefix("recver ")?.trim().parse().ok()).unwrap_or(0);
    while let Some(s) = snaps.next() {
        // gapC r19: a reloaded map (ClearEverything: MapLoaderBehaviour loads it again) has new objects: the PM's map
        // records are the earlier map's, gone from the world. The first snapshot written with the map again (Dump.NoMap
        // off) holds the live one's; the per-case snapshots after it take theirs.
        if s.pt != "PM" && pm.as_ref().is_some_and(|pm| reloaded(pm, &s)) {
            pm = Some(s.clone());
        }
        // gapB r26: a map joint made since the PM (a pin on Root) that the per-case snapshots leave out (recordings before gapC
        // r19's MapSkip by joint id: gapa16_default's Root#-97258): the last full snapshot's record of it goes in the PM's.
        if let Some(pm) = pm.as_mut().filter(|_| s.pt != "PM" && s.of("Clone").any(|r| r.get("root") == Some("Map"))) {
            let have: std::collections::HashSet<(String, i32)> = pm.records.iter().map(|r| (r.ty.clone(), r.id)).collect();
            let new: Vec<_> = s.records.iter().filter(|r| r.ty.starts_with("Map") && r.ty.ends_with("Joint2D") && !have.contains(&(r.ty.clone(), r.id))).cloned().collect();
            pm.records.extend(new);
        }
        match s.pt.as_str() {
            "PM" => pm = Some(s.clone()),
            "PI" => {
                let s = with_map(&s, pm.as_ref());
                let r = s.of("FnCase").next().ok_or("PI without an FnCase record")?;
                if !in_shard(&r.name) || SUPERSEDED.contains(&(stem, r.name.as_str())) {
                    pending = None;
                    continue;
                }
                // A Use signal (the recorder's "signal": channel 0, direct) is no rewrite argument: the item scripts' Use takes none.
                let next: Option<i32> = r.get("next").and_then(|v| v.parse().ok());
                let case = Case { s: &s, id: r.id, sid: r.get("self").and_then(|v| v.parse().ok()).unwrap_or(r.id), args: split_args(r.get("args").unwrap_or("")).into_iter().filter(|a| *a != "signal").collect(), liq: r.get("liquid").and_then(|n| super::super::circulation::LIQUIDS.iter().position(|(id, _)| *id == n.replace('_', " "))), ver };
                let case_name = r.name.clone();
                // A skipped tool call leaves the ToolController line as it was: the mutant's tline= is the FnCase's tpre= line.
                let mut_ret = r.get("tpre").and_then(|t| t.split_once('|')).map_or(String::new(), |(_, l)| format!(" tline={l}"));
                let run = match super::fnbigb::drained(&r).or_else(|| CALLS.iter().chain(super::fnbigb::BIGB_CALLS).chain(super::fnitems::ITEM_CALLS).chain(fnbiga::CALLS).chain(fnv52::CALLS).chain(GEN.iter()).find(|(n, _)| *n == r.name)) {
                    None => Err("no rewrite function listed".to_string()),
                    // A panic (an unimplemented! path, a failed expect) is this case's failure, not the whole run's.
                    Some((_, call)) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        Game::from_snapshot(&s, rig).and_then(|mut g| {
                            g.load_layers(&header);
                            // A probe case starts from the recorded state as is: the pools' timers and the renderers' sorting (a replay
                            // gets those from its own path).
                            g.load_recorded_sorting_and_pools(&s);
                            load_chunks(&mut g, &s)?;
                            load_grid_cells(&mut g, &s)?;
                            load_camera_shake(&mut g, &s)?;
                            load_undo(&mut g, &s)?;
                            load_human_counter(&mut g, &s)?;
                            load_tool_wheel_listeners(&mut g, &s)?;
                            load_freeze_stack(&mut g, &s)?;
                            load_tool_wheel(&mut g, &s)?;
                            load_map_light_blocks(&mut g, &s);
                            // Input.mousePosition and the camera (screen=), for the scripts that read the screen.
                            g.load_input(&s);
                            g.load_powers(&s);
                            // gapA r5: the recorder calls OnWillRenderObject on the target itself: the will-render loops run it alone.
                            if case_name.ends_with(".OnWillRenderObject") {
                                g.fn_render_only = Some(case.sid);
                            }
                            // Global.MousePosition (PointerBehaviour.GetMouseAngle): the recorded mouse=, when the case has one.
                            if s.snap.iter().any(|(k, _)| k == "mouse") {
                                g.MousePosition = g.input.mouse;
                            }
                            // The recorder's probe id just before the call: the game's next instance ids continue after it.
                            if let Some(next) = next {
                                g.next_instance_id = next - 2;
                            }
                            // gapC r7: the Main scene's thermal display (no snapshot record builds it); the recorder calls the
                            // methods directly, so it is active.
                            // gapC r8: the Main scene's singletons a recording lacks (scene.rs load_main_singletons: r7's thermal display,
                            // generalised); the display shows in the game's Main as recorded for its own cases.
                            {
                                let mut db = g.assets.take().ok_or("no game data")?;
                                let r = g.load_main_singletons(&mut db);
                                g.assets = Some(db);
                                r?;
                                // gapC r16: DistantSoundBehaviour.AudioSource under the game's id (`SceneAudioSource Distant_sound_behaviour`),
                                // so a Shoot's distant PlayOneShot logs against the recorded one.
                                if let (Some(a), Some(x)) = (g.distant_audio_source, s.of("SceneAudioSource").find(|x| x.name == "Distant_sound_behaviour"))
                                    && a != x.id && let Some(src) = g.audio.sources.remove(&a)
                                {
                                    g.audio.sources.insert(x.id, src);
                                    g.distant_audio_source = Some(x.id);
                                }
                                if case_name.starts_with("ThermalVisionTempDisplayBehaviour.") && let Some(d) = g.thermal_display.as_mut() { d.active = true }
                                // gapC r19: its IgnoreCameraZoom's localScale (`IgnoreCameraZoom scale=`, recordings since r19).
                                if let (Some(d), Some(v)) = (g.thermal_display.as_ref().map(|d| d.renderer), s.of("IgnoreCameraZoom").next().and_then(|r| r.get("scale"))) {
                                    let v = super::floats::<3>(v);
                                    g.renderers[d].chain[0].s = [v[0], v[1], v[2], 0.0];
                                }
                                // gapC r20: and its SizeMultiplier (`mult=` bits, recordings since r20): a driven case changes it.
                                if let Some(m) = s.of("IgnoreCameraZoom").next().and_then(|r| r.get("mult")).and_then(|v| u32::from_str_radix(v, 16).ok())
                                    && let Some(d) = g.thermal_display.as_mut()
                                {
                                    d.size_multiplier = f32::from_bits(m);
                                }
                            }
                            // gapC r8: the self's ParticleSystem fields before the call (FnTiny " pspre=<field>:<isEmitting><isPlaying>"):
                            // ours' system of that field takes the recorded isEmitting (a Play or Stop, as ItemParticleMachine's load).
                            // gapC r11: the third char (recordings since r11) is emission.enabled: ours' takes it, and a playing system with its
                            // emission off ("010") is played, not stopped.
                            for (f, v) in r.get("pspre").map(ps_fields).unwrap_or_default() {
                                // fnrec r2: a power tool's effect: ours' playing is the recorded isEmitting (fnrec r3; was isPlaying, which
                                // stays 1 after Stop while particles live). fnrec r3: the 4th char (recordings since r3) is the effect
                                // GameObject's activeInHierarchy (Play does nothing on an inactive one).
                                if f == "effect" && let Ok(k) = case.self_kind(crate::game::power::PowerKind::of_tool)
                                    && let (Some(p), Some(&b)) = (g.powers.iter_mut().find(|p| p.kind == k), v.as_bytes().first())
                                {
                                    p.playing = b == b'1';
                                    p.effect_inactive = v.as_bytes().get(3) == Some(&b'0');
                                }
                                // gapC r18: a BleedingParticleBehaviour's BloodParticleSystem (Awake's GetComponent, no serialized field): ours
                                // keeps its emission.enabled on the particle.
                                if f == "BloodParticleSystem" && let (Some(b), Some(&en)) = (bleeding_of(&g, &s, case.sid), v.as_bytes().get(2)) {
                                    g.bleeding[b].emission_enabled = en == b'1';
                                }
                                if let (Some(i), Some(e)) = (ps_field(&g, case.sid, &f), v.chars().next().filter(|x| *x != '-')) {
                                    let b = v.as_bytes();
                                    if let Some(&en) = b.get(2) {
                                        g.embedded_ps[i].emission = Some(en == b'1');
                                    }
                                    let call = if e == '1' || b.get(1..3) == Some(b"10") { crate::game::PsCall::Play } else { crate::game::PsCall::Stop(g.time.timeAsDouble) };
                                    g.ps_calls.push((i, call));
                                }
                            }
                            // gapC r9: the static fields the call can write (FnTiny " sfpre=<Class>.<field>:<value>;..."), into ours'.
                            for (f, v) in r.get("sfpre").map(ps_fields).unwrap_or_default() {
                                if let Some(x) = static_field(&mut g, &f) {
                                    x.parse(&v)?;
                                }
                            }
                            // gapB r22: the game's DateTime.Now at the call (FnTiny " clk=<month>,<day>,<TimeOfDay ticks>"): ours reads it as
                            // the fake clock. checker limit: a recording before r22 has none; ours then reads noon on 1 January (its cases ran
                            // outside 00:00-03:00 and 5 December, as their PO shows), never the sweep's own wall clock.
                            *crate::game::television::FAKE_NOW.lock().unwrap() = Some(r.get("clk").and_then(|v| {
                                let mut p = v.split(',');
                                Some((p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.parse::<i64>().ok()? as f64 / 1e7))
                            }).unwrap_or((1, 1, 12.0 * 3600.0)));
                            // gapC r7: a TelevisionBehaviour self's VideoPlayer.time (FnTiny " vtime=", double bits).
                            if let Some(v) = r.get("vtime").and_then(|v| u64::from_str_radix(v, 16).ok()) && let Some(t) = g.televisions.iter().position(|x| x.id == case.sid) {
                                g.televisions[t].video.time = f64::from_bits(v);
                            }
                            // gapB r27: ResizeHandles.IsHovering / the self PhysicalBehaviour's slidingAudioSource before the call (FnTiny " gb27pre=").
                            if let Some(v) = r.get("gb27pre") {
                                gb27_load(&mut g, case.sid, v)?;
                            }
                            // gapC r5: a gcpre=1 case (the recorder set the root's texts to "gapC" and cleared its clips): ours loads both from PI.
                            if r.get("gcpre").is_some() {
                                for t in s.of("ItemText") {
                                    let Some(i) = g.world_texts.iter().position(|w| w.id == t.id) else { continue };
                                    let v = t.get("text").unwrap_or("").trim_matches('"').replace("\\n", "\n").replace("\\r", "\r").replace('_', " ");
                                    g.text_set(i, &v);
                                }
                                if case_name.starts_with("ThermalVisionTempDisplayBehaviour.") && let Some(t) = g.thermal_display.as_ref().map(|t| t.text) {
                                    g.text_set(t, "gapC");
                                }
                                if case_name.starts_with("LayerEditorController.") {
                                    g.layer_editor.LayerDisplay = "gapC".into();
                                    g.layer_editor.OrderDisplay = "gapC".into();
                                }
                                // gapC r6: the recorder set the root's sprites to other sprites the item holds: ours loads each by name.
                                for r in s.of("ItemSprite") {
                                    let Some(n) = r.get("sprite").filter(|n| *n != "null") else { continue };
                                    let Some(o) = g.assets.as_ref().and_then(|a| a.sprite_by_name(n).or_else(|| a.sprite_by_name(&n.replace('_', " ")))) else { continue };
                                    match g.item_sprite(r.id) {
                                        Some(crate::game::visual::SpriteAt::Physical(p)) => { if let Some(x) = g.physicals[p].sprite.as_mut() { x.0 = o } }
                                        Some(crate::game::visual::SpriteAt::Renderer(i)) => g.renderers[i].sprite.0 = o,
                                        None => {}
                                    }
                                }
                            }
                            // gapA r11: FnTiny.ToolPre's tlr=1: the tool's own lineRenderer field is set (its OnToolChosen ran once, the
                            // same component chosen on an earlier case), so its OnSelect/OnHold don't throw while it is not chosen.
                            if case.field("tlr") == Some("1") {
                                match case.field("selfty") {
                                    Some("SerpentineBeltTool") => g.belt_tool.created = true,
                                    Some("LinkDeviceTool" | "PhaseLinkTool") => g.link_tool.created = true,
                                    _ => if let Ok(k) = case.self_kind(crate::game::blood_wire::WireKind::of_tool) && !g.wire_tool.created.contains(&k) { g.wire_tool.created.push(k) },
                                }
                            }
                            // gapA r9: a Main scene component (a positive level1 id) the rewrite keeps under its singleton's id: the call takes that id.
                            let alias = |x: i32| g.scene_alias.get(&x).copied().unwrap_or(x);
                            let case = Case { s: case.s, id: alias(case.id), sid: alias(case.sid), args: case.args.clone(), liq: case.liq, ver: case.ver };
                            // gapB r15: the self's reference caches (ref_cache.rs: the fields its Awake/Start fills at top level with
                            // GetComponent/Find) reset to null first, so the call has to fill them again; ref_reset_diffs compares them.
                            if let Some((_, names)) = super::ref_cache::REF_CACHE.iter().find(|(n, _)| *n == case_name) && let Some((class, r)) = super::clone::script_records(&s).find(|(_, r)| r.id == case.sid) {
                                crate::game::REF_LIVE.with(|l| l.set(true));
                                let mut reset = Vec::new();
                                if let Some((_, links)) = super::fnitems::item_fields(&mut g, class, case.sid) {
                                    for (name, field) in links {
                                        if let Some(pre) = r.get(name).filter(|_| names.contains(&name)) && field.parse("null").is_ok() {
                                            reset.push((class.to_string(), case.sid, name, pre.to_string()));
                                        }
                                    }
                                }
                                crate::game::REF_LIVE.with(|l| l.set(false));
                                g.ref_reset = reset;
                            }
                            ours_pre_clips = g.audio.sources.keys().map(|&i| (i, clip_of(&g, i))).collect();
                            ours_pre_vols = g.audio.sources.keys().map(|&i| (i, vol_of(&g, i))).collect();
                            ours_pre_seq = g.coroutine_seq;
                            g.audio.shot_log.clear();
                            ours_pre_clone = clone_look(&g, case.sid);
                            ours_pre_gos = ours_gos(&g);
                            ours_pre_next = g.next_instance_id;
                            ours_pre_look = s.of("ItemSprite").map(|r| ((false, r.id), sprite_of(&g, r.id))).chain(s.of("ItemText").map(|r| ((true, r.id), text_of(&g, r.id)))).collect();
                            ours_pre_mpb = s.of("ItemSprite").filter_map(|r| Some((r.id, ours_mpb(&g, r.id)?))).collect();
                            ours_pre_order = s.of("ItemSprite").filter_map(|r| Some((r.id, ours_order(&g, r.id)?))).collect();
                            ours_pre_phys = s.of("PhysicalBehaviour").filter_map(|r| Some((r.id, phys_links(&g, r.id)?))).collect();
                            ours_pre_menu = s.of("PhysicalBehaviour").filter_map(|r| Some((r.id, menu_ours(&g, r.id)?))).collect();
                            ours_pre_ignored = g.world.ignored.clone();
                            drag_pre = g.tool.as_ref().map_or(vec![], |t| t.destroying.iter().chain(&t.dragJoint).map(|j| (j.id, j.destroy_at)).collect());
                            gore_pre = g.limbs.iter().map(|l| (l.gore.spring_id, l.gore.destroy_at)).collect();
                            limb_hinge_pre = g.limbs.iter().filter_map(|l| {
                                let h = l.Joint?;
                                let name = &g.physicals[l.PhysicalBehaviour].name;
                                Some((h, s.of("HingeJoint2D").find(|r| &r.name == name)?.id, g.hinges[h].destroy_at))
                            }).collect();
                            ours_pre_destroys = (g.component_destroys.clone(), g.object_destroys.clone(), g.joint_destroys.len(), g.attachment_joint_destroys.len(), g.glows.len());
                            #[cfg(fncov)]
                            cov::start();
                            // A managed exception (menu::throw) is the call's result, the recorder's `exc:<class>`.
                            let mut ret = None;
                            let ret = match super::super::menu::catch_managed(|| ret = Some(if mutated(&case_name) { Ok(mut_ret.clone()) } else { call(&mut g, &case) })) {
                                Some(ex) => Ok(format!("exc:{}", ex.0.split(':').next().unwrap_or(""))),
                                None => ret.unwrap(),
                            };
                            #[cfg(fncov)]
                            cov::stash();
                            Ok((g, ret?))
                        })
                    }))
                    .unwrap_or_else(|e| Err(format!("panic: {}", e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()))),
                };
                pending = Some((s.clone(), run));
            }
            "PO" => {
                let Some((pi, run)) = pending.take() else { continue };
                // "SameAsPI": the recorder found the PO's records equal to the PI's and wrote only its snap line.
                let s = if s.of("SameAsPI").next().is_some() {
                    let mut records: Vec<_> = pi.records.iter().filter(|r| r.ty != "FnCase").cloned().collect();
                    records.extend(s.records.iter().filter(|r| r.ty != "SameAsPI").cloned());
                    Snapshot { pt: s.pt.clone(), snap: s.snap.clone(), records }
                } else {
                    with_map(&s, pm.as_ref())
                };
                let r = s.of("FnResult").next().ok_or("PO without an FnResult record")?;
                let name = pi.of("FnCase").next().unwrap().name.clone();
                // A tool's void GetEndObject in a recording made before the recorder read its endBody (ret=void): no game answer.
                if name.ends_with(".GetEndObject") && r.get("ret") == Some("void") {
                    continue;
                }
                let diffs = match run {
                    Err(e) => vec![e],
                    Ok((mut g, ours)) => {
                        let theirs = r.get("ret").unwrap_or("");
                        let theirs = if theirs == "void" { "" } else { theirs };
                        // An out arg: the rewrite's string carries " out=<args after>".
                        // gapC r4: a non-empty game em=/muz=/teff= ours lacks (a skipped call, a rewrite that never gives it) is a difference, not
                        // a skipped compare. Not out=: older recorders echo the args there for every call (fni2_elec, fnprobe3).
                        let theirs = if ours.contains(" out=") { format!("{theirs} out={}", r.get("out").unwrap_or("")) } else { theirs.to_string() };
                        // A BallisticsEmitter's state after the call (BigProbeB.After: " em=" after a non-void ret, its own field).
                        let theirs = if ours.contains(" em=") || r.get("em").is_some_and(|v| !v.is_empty()) { format!("{theirs} em={}", r.get("em").unwrap_or("")) } else { theirs };
                        // CirculationBehaviour.OnImpactCreated's SetColor result (fnbatch r6, Recorder.cs " dcm=").
                        let theirs = match r.get("dcm") { Some(v) => format!("{theirs} dcm={v}"), None => theirs };
                        // ConditionalTextBehaviour.Update's t and text after the call (batch1 r7, Recorder.cs " ctt=").
                        let theirs = match r.get("ctt") { Some(v) => format!("{theirs} ctt={v}"), None => theirs };
                        // gapC r6: a LayerEditorController's LayerDisplay|OrderDisplay texts after the call (Recorder.cs " led=").
                        // checker limit: led= only when ours knows the texts before the call (a gcpre=1 seed or BigProbeB's em= LeState);
                        // a plain case's PI holds no LayerDisplay/OrderDisplay (gapC r26: gapa15_default's led=15|15 vs ours empty).
                        let led_known = pi.of("FnCase").next().is_some_and(|x| x.get("gcpre").is_some() || x.get("em").is_some());
                        let theirs = match r.get("led").filter(|_| led_known) { Some(v) => format!("{theirs} led={v}"), None => theirs };
                        let ours = if r.get("led").is_some() && led_known { format!("{ours} led={}|{}", g.layer_editor.LayerDisplay.replace(' ', "_"), g.layer_editor.OrderDisplay.replace(' ', "_")) } else { ours };
                        // gapC r7: ThermalVisionTempDisplayBehaviour.Text after the call (Recorder.cs " tvt=").
                        let theirs = match r.get("tvt") { Some(v) => format!("{theirs} tvt={v}"), None => theirs };
                        let ours = if r.get("tvt").is_some() { format!("{ours} tvt={}", g.thermal_display.as_ref().map_or("null".into(), |t| format!("\"{}\"", g.text_get(t.text).replace(' ', "_").replace('\n', "\\n")))) } else { ours };
                        // gapB r24: a BlasterboltBehaviour.Update self's Trail.autodestruct after the call (Recorder.cs " tad=").
                        let theirs = match r.get("tad") { Some(v) => format!("{theirs} tad={v}"), None => theirs };
                        let ours = match (r.get("tad"), pi.of("FnCase").next().and_then(|c| c.get("self")).and_then(|v| v.parse::<i32>().ok())) {
                            (Some(_), Some(sid)) => format!("{ours} tad={}", g.blasterbolts.iter().find(|x| x.id == sid).map_or(0, |b| b.trail_autodestruct as u8)),
                            _ => ours,
                        };
                        // gapB r27: ResizeHandles.IsHovering / the slidingAudioSource after the call (Recorder.cs " gb27=").
                        let theirs = match r.get("gb27") { Some(v) => format!("{theirs} gb27={v}"), None => theirs };
                        let ours = match (r.get("gb27"), pi.of("FnCase").next().and_then(|c| c.get("self")).and_then(|v| v.parse::<i32>().ok())) {
                            (Some(v), Some(sid)) => format!("{ours} gb27={}", gb27_state(&g, sid, v.starts_with("rh:"))),
                            _ => ours,
                        };
                        // gapB r27: an UnderwaterExplosionBehaviour.Start self's DelayedSplash delay (Recorder.cs " uwd="; none: not started).
                        let theirs = match r.get("uwd") { Some(v) => format!("{theirs} uwd={v}"), None => theirs };
                        let ours = match (r.get("uwd"), pi.of("FnCase").next().and_then(|c| c.get("self")).and_then(|v| v.parse::<i32>().ok())) {
                            (Some(_), Some(sid)) => {
                                let i = g.underwater_explosions.iter().position(|u| u.id == sid);
                                let d = g.coroutines.iter().rev().find(|x| matches!(x.step, crate::game::destroyable::Step::UnderwaterSplash(j) if Some(j) == i))
                                    .map_or("none".to_string(), |x| format!("{:08x}", ((x.at - g.time.dynamicTimeAsDouble) as f32).to_bits()));
                                format!("{ours} uwd={d}")
                            }
                            _ => ours,
                        };
                        // gapB r25: an ExplosionSoundBehviour.Awake self's audioSource pitch after the call (Recorder.cs " esp=").
                        let theirs = match r.get("esp") { Some(v) => format!("{theirs} esp={v}"), None => theirs };
                        let ours = match (r.get("esp"), pi.of("FnCase").next().and_then(|c| c.get("self")).and_then(|v| v.parse::<i32>().ok())) {
                            (Some(_), Some(sid)) => {
                                let src = g.audio.source_field(sid, "audioSource").or_else(|| g.audio.object_source(sid));
                                format!("{ours} esp={}", src.and_then(|s| g.audio.sources.get(&s)).map_or("null".to_string(), |s| format!("{:08x}", s.pitch.to_bits())))
                            }
                            _ => ours,
                        };
                        // AntennaReceiverBehaviour.GetNextFloat's rand after the call (batch1 r8, Recorder.cs " rnd=").
                        let theirs = match r.get("rnd") { Some(v) => format!("{theirs} rnd={v}"), None => theirs };
                        // BurnableSpriteBehaviour.OnWillRenderObject's block _Progress after the call (batch1 r9, Recorder.cs " bsp=").
                        let theirs = match r.get("bsp") { Some(v) => format!("{theirs} bsp={v}"), None => theirs };
                        // BurnableSpriteBehaviour.Awake's new propertyBlock (batch1 r10, Recorder.cs " bsa=").
                        let theirs = match r.get("bsa") { Some(v) => format!("{theirs} bsa={v}"), None => theirs };
                        // ChainsawBehaviour.OnBeforeSerialise's SerialisableVictims (batch1 r10, Recorder.cs " sv=").
                        let theirs = match r.get("sv") { Some(v) => format!("{theirs} sv={v}"), None => theirs };
                        // ContraptionOutlineSerialiser.SaveOutline's written file (batch1 r12, Recorder.cs " olw=").
                        let theirs = match r.get("olw") { Some(v) => format!("{theirs} olw={v}"), None => theirs };
                        // ExplosionCreator.CreateUnderwaterExplosionEffect's new UnderwaterExplosion (gapB r1, Recorder.cs " uw=").
                        let theirs = match r.get("uw") { Some(v) => format!("{theirs} uw={v}"), None => theirs };
                        // gapB r19: ModAPI.Invoke<X>'s probe subscriber receipts (Recorder.cs " mrx=").
                        let theirs = match r.get("mrx") { Some(v) => format!("{theirs} mrx={v}"), None => theirs };
                        // gapC r2: a tool self's ToolController LineRenderer after the call (FnTiny.ToolLine, " tline="), when ours gives one.
                        let theirs = if ours.contains(" tline=") { format!("{theirs} tline={}", r.get("tline").unwrap_or("")) } else { theirs };
                        // gapC r2: BlasterBehaviour.Start's Muzzleflash rotation (" muz="), AOEPowerTool.Awake's effect (" teff=").
                        let theirs = if ours.contains(" muz=") || r.get("muz").is_some_and(|v| !v.is_empty()) { format!("{theirs} muz={}", r.get("muz").unwrap_or("")) } else { theirs };
                        let theirs = if ours.contains(" teff=") || r.get("teff").is_some_and(|v| !v.is_empty()) { format!("{theirs} teff={}", r.get("teff").unwrap_or("")) } else { theirs };
                        // gapC r28: the private state after the call (FnTiny.StateOf, " st="), for an entry that gives ours.
                        // A skipped call (mutant) has no st=: the game's state after still counts.
                        let theirs = if ours.contains(" st=") || (mutated(&name) && r.get("st").is_some()) { format!("{theirs} st={}", r.get("st").unwrap_or("")) } else { theirs };
                        let mut d = Vec::new();
                        // A Vector3 the rewrite keeps as [f32; 2]: a zero z.
                        // A RagdollPose from a recorder before batch3 r3: its name only.
                        let ours = if theirs.starts_with("RagdollPose:") && !theirs.contains("/rigidity=") { ours.split("/rigidity=").next().unwrap().to_string() } else { ours };
                        let ours = if theirs.matches(',').count() == 2 && ours.matches(',').count() == 1 && (theirs.ends_with(",00000000") || theirs.ends_with(",80000000")) { format!("{ours},{}", &theirs[theirs.len() - 8..]) } else { ours };
                        // An index return (the rewrite's arena index for the game's object), a Vector2Int or a ValueTuple pair.
                        let theirs = if ours != theirs { game_ret_plain(&g, &pi, &theirs) } else { theirs };
                        if ours != theirs {
                            d.push(format!("return: game {theirs} ours {ours}"));
                        }
                        g.sync_pending();
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            let mut d = compare(&mut g, &s);
                            d.extend(g.compare_item_scripts(&s));
                            d.extend(ref_reset_diffs(&mut g, &s));
                            d.extend(super::fnweap::compare_wires(&g, &s));
                            // Always on since gapA r4 (a sweep of the 181 recordings PASS rows name, as is and with both compares:
                            // no function that passed failed with them). PPG_FNCHECK_NOJOINTS=1 turns both off for a comparison.
                            let joints = std::env::var_os("PPG_FNCHECK_NOJOINTS").is_none();
                            d.extend(super::fnweap::compare_joints(&g, &s, joints, joints));
                            d.extend(compare_grid_cells(&g, &s));
                            d.extend(compare_camera_shake(&g, &s));
                            d.extend(compare_ignore_stack(&g, &s));
                            d.extend(compare_undo(&g, &s));
                            d.extend(compare_tool_wheel_listeners(&g, &s));
                            d.extend(compare_freeze_stack(&g, &s));
                            d.extend(compare_tool_wheel(&g, &pi, &s));
                            d.extend(compare_map_light_blocks(&g, &s));
                            // gapA r11: a tool self's effect (FnTiny.ToolPre's selfty=): the ToolController's drag state and joint, and the
                            // chosen wire/link/belt tool's startPos/startOffset/hasDragged (its line's SetPositions read them).
                            // gapA r13: and a ToolControllerBehaviour/SelectionController self (FnTiny's hover/selection seed): the
                            // hover, the selection and isSelecting they write (DetermineHovering, EscapeAndDeselect, StopSelectionDrag).
                            if pi.of("FnCase").next().and_then(|c| c.get("selfty")).is_some() || name.starts_with("ToolControllerBehaviour.") || name.starts_with("SelectionController.") {
                                d.extend(super::compare_tool(&g, &s).unwrap_or_else(|e| vec![format!("tool compare: {e}")]));
                                // fnrec r2: every recorded power tool's mouseMovement and oldMousePos (FixedUpdate runs them all).
                                for r in s.records.iter().filter(|r| crate::game::power::PowerKind::of_tool(&r.ty).is_some()) {
                                    let k = crate::game::power::PowerKind::of_tool(&r.ty).unwrap();
                                    let Some(p) = g.powers.iter().find(|p| p.kind == k) else { d.push(format!("{} #{}: ours has no power", r.ty, r.id)); continue };
                                    let h = |v: [f32; 2]| format!("{:08x},{:08x}", v[0].to_bits(), v[1].to_bits());
                                    for (f, ours) in [("mouseMovement", h(p.mouseMovement)), ("oldMousePos", h(p.oldMousePos))] {
                                        if let Some(t) = r.get(f) && *t != ours { d.push(format!("{} #{} {f}: game {t} ours {ours}", r.ty, r.id)) }
                                    }
                                }
                            }
                            // batch4 r8: HandleToolSelectionInput's effects compare() leaves out: the cycle fields and CurrentTool's type
                            // (the PO snap's tool=; ours the ToolLibrary entry SetTool chose last, else the PI's tool).
                            if name == "ToolControllerBehaviour.HandleToolSelectionInput" {
                                if let (Some(r), Some(t)) = (s.of("ToolControllerBehaviour").next(), g.tool.as_ref()) {
                                    for (k, ours) in [("parentCycleIndex", t.parentCycleIndex), ("childCycleIndex", t.childCycleIndex), ("childCount", t.childCount)] {
                                        if r.get(k) != Some(ours.to_string().as_str()) { d.push(format!("{k}: game {} ours {ours}", r.get(k).unwrap_or("?"))) }
                                    }
                                }
                                let tool = |s: &Snapshot| s.snap.iter().find(|(k, _)| k == "tool").map(|(_, v)| v.clone()).unwrap_or_default();
                                let chosen = g.tool.as_ref().map(|t| t.chosen.clone()).unwrap_or_default();
                                let ours = if chosen.is_empty() { tool(&pi) } else {
                                    let (a, b) = (g.tool_collection(0), g.tool_collection(1));
                                    a.iter().chain(b.iter()).find(|e| e.name == chosen).map_or(format!("?{chosen}"), |e| e.ty.clone())
                                };
                                if tool(&s) != ours { d.push(format!("CurrentTool: game {} ours {ours}", tool(&s))) }
                            }
                            // batch4 r8: SignalConverterBehaviour.Use/UseContinuous: a wired converter's UseWireBehaviour only starts
                            // SendUse(Continuous)Message, which waits: each item UseWireBehaviour record's wait= (its wake less
                            // Time.timeAsDouble, "none") against our latest routine wait of that wire.
                            // gapA r17: every other call too (the continuous-signal family, OnContinuousUpdate -> SendAllChannelContinuous
                            // IsolatedActivation -> the wire's UseContinuous), when the call changed the game's wait (PI against PO) or
                            // started one of ours: only where the PI's wait is none (the seed forgets the recorder's old wakes; the
                            // routines themselves still run, and ours loads them), against ours' routine started during the call.
                            let conv = name == "SignalConverterBehaviour.Use" || name == "SignalConverterBehaviour.UseContinuous";
                            for r in s.of("ItemScript").filter(|r| r.get("class") == Some("UseWireBehaviour")) {
                                let Some(theirs) = r.get("wait") else { continue };
                                let pre = pi.of("ItemScript").find(|x| x.id == r.id).and_then(|x| x.get("wait")).unwrap_or("none");
                                if !conv && pre != "none" { continue }
                                let ours = g.coroutines.iter().filter(|c| c.owner == r.id && (conv || c.seq > ours_pre_seq) && matches!(c.step, crate::game::destroyable::Step::Routine(..))).max_by_key(|c| c.seq)
                                    .map_or("none".to_string(), |c| format!("{:08x}", ((c.at - g.time.dynamicTimeAsDouble) as f32).to_bits()));
                                if theirs != ours { d.push(format!("UseWireBehaviour #{} wait: game {theirs} ours {ours}", r.id)) }
                            }
                            // batch2 r7: the LightningTool's entity activeSelf (`LightningToolState active=`, GenRun since r7).
                            if let (Some(r), Some(e)) = (s.of("LightningToolState").next(), g.lightning_tool.as_ref().map(|t| t.entity)) {
                                let ours = if g.lightning_entity_active(e) { "1" } else { "0" };
                                if r.get("active") != Some(ours) {
                                    d.push(format!("LightningToolState active: game {} ours {ours}", r.get("active").unwrap_or("?")));
                                }
                            }
                            // gapC r4/r5: an AudioSource's clip (`ItemAudioSource clip=`, PI against PO), ours by name: compared when the call
                            // changed it in the game, or when it changed ours (ours before the call against after) while the game's stayed.
                            for r in s.of("ItemAudioSource").chain(s.of("SceneAudioSource")) {
                                let name = |v: Option<&str>| v.map(|c| c.strip_prefix('@').and_then(|c| c.rsplit_once('#')).map_or(c.to_string(), |(n, _)| n.to_string()));
                                let (theirs, pre) = (name(r.get("clip")), name(pi.of(&r.ty).find(|x| x.id == r.id).and_then(|x| x.get("clip"))));
                                let Some(theirs) = theirs.filter(|_| g.audio.sources.contains_key(&r.id)) else { continue };
                                let ours = clip_of(&g, r.id);
                                let changed = Some(&theirs) != pre.as_ref() || ours_pre_clips.get(&r.id).is_some_and(|p| *p != ours);
                                if changed && theirs != ours { d.push(format!("AudioSource {} #{} clip: game {theirs} ours {ours}", r.name, r.id)) }
                            }
                            // v52h: volume, minDistance and maxDistance (a recording made with PPG_AUDIO_VOL=1), by the same rule.
                            for r in s.of("ItemAudioSource") {
                                if !g.audio.sources.contains_key(&r.id) { continue }
                                let ours = vol_of(&g, r.id);
                                let pre = pi.of(&r.ty).find(|x| x.id == r.id);
                                for (k, key) in ["vol", "mind", "maxd"].iter().enumerate() {
                                    let Some(theirs) = r.get(key) else { continue };
                                    let changed = pre.and_then(|x| x.get(key)) != Some(theirs) || ours_pre_vols.get(&r.id).is_some_and(|w| w[k] != ours[k]);
                                    if changed && theirs != ours[k] { d.push(format!("AudioSource {} #{} {key}: game {theirs} ours {}", r.name, r.id, ours[k])) }
                                }
                            }
                            // gapC r16: the PhysicalBehaviour members compare() leaves out, by the same rule (ToggleableMirrorBehaviour's
                            // ReflectsLasers/AbsorbsLasers, OnCollisionStayWithoutSleep's collisionBufferCount).
                            for r in s.of("PhysicalBehaviour") {
                                let (Some(ours), Some(p)) = (phys_links(&g, r.id), pi.of("PhysicalBehaviour").find(|x| x.id == r.id)) else { continue };
                                for (k, key) in PHYS_LINKS.iter().enumerate() {
                                    let Some(theirs) = r.get(key) else { continue };
                                    let changed = p.get(key) != Some(theirs) || ours_pre_phys.get(&r.id).is_some_and(|w| w[k] != ours[k]);
                                    if changed && theirs != ours[k] { d.push(format!("PhysicalBehaviour {} #{} {key}: game {theirs} ours {}", r.name, r.id, ours[k])) }
                                }
                                // gapA r18: its context menu, by the same rule.
                                let (Some(theirs), Some(ours)) = (menu_rec(&s, r), menu_ours(&g, r.id)) else { continue };
                                let changed = menu_rec(&pi, p).as_ref() != Some(&theirs) || ours_pre_menu.get(&r.id).is_some_and(|w| *w != ours);
                                // On by default since gapA r21 (PPG_FNCHECK_MENU=0 turns it off; menusweep r20m/r21m found no menu-only FAIL left). gapA r18: ours' loaded menu lacks the buttons the recorded Starts added, so whole
                                // lists differ on every Start case (gapa15_default); compare what the call appended instead.
                                // gapA r19: the buttons the call appended (game PO after PI, ours after ours' pre), when both only appended.
                                let pre_t = menu_rec(&pi, p);
                                match (menu_appended(pre_t.as_deref(), &theirs), menu_appended(ours_pre_menu.get(&r.id).map(|x| x.as_str()), &ours)) {
                                    (Some(a), Some(o)) => if a != o && std::env::var("PPG_FNCHECK_MENU").map_or(true, |v| v != "0") { d.push(format!("PhysicalBehaviour {} #{} context menu appended: game [{}] ours [{}]", r.name, r.id, a.join(";"), o.join(";"))) },
                                    _ => if changed && theirs != ours && std::env::var("PPG_FNCHECK_MENU").map_or(true, |v| v != "0") { d.push(format!("PhysicalBehaviour {} #{} context menu: game [{theirs}] ours [{ours}]", r.name, r.id)) },
                                }
                            }
                            // gapC r19: the Main scene's IgnoreCameraZoom localScale (OnWillRenderObject), when the call changed it in the
                            // game or in ours (ours before the call is the PI's).
                            if let (Some(r), Some(rd)) = (s.of("IgnoreCameraZoom").next(), g.thermal_display.as_ref().map(|t| t.renderer)) {
                                let ours = vec::<3>(g.renderers[rd].chain[0].s[..3].try_into().unwrap());
                                let pre = pi.of("IgnoreCameraZoom").next().and_then(|x| x.get("scale"));
                                if let (Some(theirs), Some(pre)) = (r.get("scale"), pre) && (theirs != pre || ours != pre) && theirs != ours {
                                    d.push(format!("IgnoreCameraZoom {} #{} localScale: game {theirs} ours {ours}", r.name, r.id));
                                }
                            }
                            // gapC r19: Physics2D.IgnoreCollision's pairs (`IgnoredCollisions pairs=`, min,max collider ids): each pair the
                            // call added or removed in the game or in ours (IgnoreCollisionStackController's substitute) must agree.
                            if let (Some(theirs), Some(pre)) = (ignored_pairs(&s), ignored_pairs(&pi)) {
                                let ours = &g.world.ignored;
                                let changed: std::collections::BTreeSet<(i32, i32)> = theirs.symmetric_difference(&pre).chain(ours.symmetric_difference(&ours_pre_ignored)).copied().collect();
                                // fc41 r5: checker limit: the recorder lists pairs of FindObjectsOfType<Collider2D> (active objects only), so a pair
                                // with an inactive collider (EnergySword's BladeObject while off) is never in the game's list. Such a pair is
                                // left out when both stacks hold it (the stack counts are compared on their own).
                                let blind = |k: &(i32, i32)| !theirs.contains(k) && g.ignore_stack.get(k).is_some_and(|n| *n > 0) && s.of("IgnoreStack").next().and_then(|r| r.get("pairs")).is_some_and(|v| v.trim_matches(['[', ']']).split(';').any(|x| x.rsplit_once(':').is_some_and(|(p, n)| p == format!("{},{}", k.0, k.1) && n != "0")));
                                for k in changed.into_iter().filter(|k| theirs.contains(k) != ours.contains(k) && !blind(k)) {
                                    d.push(format!("IgnoreCollision {},{}: game {} ours {}", k.0, k.1, bit(theirs.contains(&k)), bit(ours.contains(&k))));
                                }
                            }
                            // gapC r18: a Rigidbody2D's sharedMaterial set or not (`material=`; PhysicalBehaviour.SetProperties sets a null one),
                            // compared when the call changed it in the game or in ours (ours before the call is the PI's: the loader's).
                            for r in s.of("Rigidbody2D").chain(s.of("ItemRigidbody2D")) {
                                let (Some(theirs), Some(pre)) = (r.get("material").map(|v| v != "null"), pi.of(&r.ty).find(|x| x.id == r.id).and_then(|x| x.get("material")).map(|v| v != "null")) else { continue };
                                let Some(ours) = g.physicals.iter().find(|p| p.rigidbody.component == r.id && !p.destroyed).map(|p| p.rigidbody_has_material) else { continue };
                                if (theirs != pre || ours != pre) && theirs != ours { d.push(format!("Rigidbody2D {} #{} sharedMaterial: game {} ours {}", r.name, r.id, r.get("material").unwrap_or(""), if ours { "set" } else { "null" })) }
                            }
                            // gapC r5: the same rule for a SpriteRenderer's sprite and a TextMeshPro's text (recordings since gapC r5).
                            for (text, ty, key) in [(false, "ItemSprite", "sprite"), (true, "ItemText", "text")] {
                                for r in s.of(ty) {
                                    // gapC r8: a text the call made (in PO, not in PI: GaugeBehaviour.CreateLabels' label clones) that ours lacks.
                                    if text && r.get(key).is_some() && text_of(&g, r.id).is_none() && !pi.of(ty).any(|x| x.id == r.id) {
                                        d.push(format!("{ty} {} #{} {key}: game {}, not ours", r.name, r.id, r.get(key).unwrap_or("")));
                                        continue;
                                    }
                                    let (Some(theirs), Some(ours)) = (r.get(key), if text { text_of(&g, r.id) } else { sprite_of(&g, r.id) }) else { continue };
                                    let pre = pi.of(ty).find(|x| x.id == r.id).and_then(|x| x.get(key));
                                    let changed = Some(theirs) != pre || ours_pre_look.get(&(text, r.id)).is_some_and(|p| p.as_ref() != Some(&ours));
                                    if changed && theirs != ours { d.push(format!("{ty} {} #{} {key}: game {theirs} ours {ours}", r.name, r.id)) }
                                }
                            }
                            // gapC r11: each SpriteRenderer's MaterialPropertyBlock (`ItemSprite mpb=`, by the shader's property names; an
                            // empty block: every name ours holds) and sortingLayerID,sortingOrder (`order=`), by the same rule.
                            for r in s.of("ItemSprite") {
                                let p = pi.of("ItemSprite").find(|x| x.id == r.id);
                                if let (Some(theirs), Some(ours)) = (r.get("mpb").and_then(mpb_map), ours_mpb(&g, r.id)) {
                                    let pre = p.and_then(|x| x.get("mpb")).and_then(mpb_map).unwrap_or_default();
                                    let was = ours_pre_mpb.get(&r.id).cloned().unwrap_or_default();
                                    let names: std::collections::BTreeSet<&String> = if r.get("mpb") == Some("-") { ours.keys().chain(was.keys()).collect() } else { theirs.keys().collect() };
                                    for n in names {
                                        let v = |m: &std::collections::BTreeMap<String, String>| m.get(n).cloned().unwrap_or_default();
                                        let (t, o) = (v(&theirs), v(&ours));
                                        if (t != v(&pre) || o != v(&was)) && t != o { d.push(format!("SpriteRenderer {} #{} block {n}: game {t} ours {o}", r.name, r.id)) }
                                    }
                                }
                                if let (Some(theirs), Some(ours)) = (r.get("order"), ours_order(&g, r.id)) {
                                    let changed = p.and_then(|x| x.get("order")) != Some(theirs) || ours_pre_order.get(&r.id).is_some_and(|w| *w != ours);
                                    if changed && theirs != ours { d.push(format!("SpriteRenderer {} #{} sorting: game {theirs} ours {ours}", r.name, r.id)) }
                                }
                            }
                            // gapC r7: the self's clone's objects: activeSelf and localRotation, compared when the call changed them in the
                            // game (PI against PO) or ours (before against after). Quaternions by value (-0 == 0).
                            {
                                let now = ours_gos(&g);
                                let pre: std::collections::HashSet<i32> = pi.of("Clone").map(|r| r.id).collect();
                                // gapA r22: every root kind the PI dumps too (Human, Map, Pasted: a wire's or a map rope's objects; an older
                                // recording's PI holds only the item's) and Spawned (the run-time roots, debris: new by definition).
                                let kind = |r: &super::Record| r.get("root").map(|k| k.split(':').next().unwrap_or(k).to_string());
                                let kinds: std::collections::HashSet<String> = pi.of("Clone").filter_map(kind).chain(["Item".into(), "Spawned".into()]).collect();
                                let made: Vec<&super::Record> = s.of("Clone").filter(|r| !pre.contains(&r.id) && kind(r).is_some_and(|k| kinds.contains(&k))).collect();
                                // gapA r22: an older recording's FnCase has no next= (ours' ids then do not follow the game's): the count
                                // of GameObjects the call made, the game's against ours' (ours' new Instance objects, or the ids it took / 2).
                                if pi.of("FnCase").next().and_then(|c| c.get("next")).is_none() {
                                    let ours = now.difference(&ours_pre_gos).count().max((ours_pre_next - g.next_instance_id).max(0) as usize / 2);
                                    if ours < made.len() {
                                        d.push(format!("Clone: the call made {} GameObjects in the game ({}), {ours} in ours", made.len(), made.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(";")));
                                    }
                                } else {
                                    for r in made {
                                        let took = r.id <= ours_pre_next && r.id > g.next_instance_id;
                                        if !took && (!now.contains(&r.id) || ours_pre_gos.contains(&r.id)) {
                                            d.push(format!("Clone {} #{}: made by the call in the game, not in ours", r.name, r.id));
                                        }
                                    }
                                }
                            }
                            if let Some(sid) = pi.of("FnCase").next().and_then(|c| c.get("self")).and_then(|v| v.parse::<i32>().ok()) {
                                let post: std::collections::HashMap<i32, &super::Record> = s.of("Clone").map(|r| (r.id, r)).collect();
                                let pre: std::collections::HashMap<i32, &super::Record> = pi.of("Clone").map(|r| (r.id, r)).collect();
                                let sa = |r: &super::Record| r.get("active").and_then(|a| a.split_once(',')).map(|x| x.1 == "1");
                                let q = |r: &super::Record| { let v: Vec<f32> = r.get("lrot")?.split(',').filter_map(|h| u32::from_str_radix(h, 16).ok().map(f32::from_bits)).collect(); <[f32; 4]>::try_from(v).ok() };
                                let v3 = |r: &super::Record, k: &str| { let v: Vec<f32> = r.get(k)?.split(',').filter_map(|h| u32::from_str_radix(h, 16).ok().map(f32::from_bits)).collect(); <[f32; 3]>::try_from(v).ok() };
                                for (go, (a, rot, ts)) in clone_look(&g, sid) {
                                    let (Some(r), Some(p)) = (post.get(&go), pre.get(&go)) else { continue };
                                    let was = ours_pre_clone.get(&go);
                                    if let Some(theirs) = sa(r) && (sa(r) != sa(p) || was.is_some_and(|w| w.0 != a)) && theirs != a {
                                        d.push(format!("Clone {} #{go} activeSelf: game {} ours {}", r.name, theirs as u8, a as u8));
                                    }
                                    if let (Some(ours), Some(theirs)) = (rot, q(r)) && (Some(theirs) != q(p) || was.is_some_and(|w| w.1 != rot)) && theirs != ours {
                                        d.push(format!("Clone {} #{go} lrot: game {theirs:?} ours {ours:?}", r.name));
                                    }
                                    if let Some((t, sc)) = ts {
                                        for (k, ours, w) in [("lpos", t, was.and_then(|w| w.2).map(|x| x.0)), ("lscale", sc, was.and_then(|w| w.2).map(|x| x.1))] {
                                            // gapC r13: a NaN component equals a NaN (TrivialAnimation's Slide holds one in both).
                                            let ne = |a: [f32; 3], b: [f32; 3]| a.iter().zip(&b).any(|(x, y)| x != y && !(x.is_nan() && y.is_nan()));
                                            if let Some(theirs) = v3(r, k) && (v3(p, k).is_none_or(|p| ne(theirs, p)) || w.is_some_and(|w| ne(w, ours))) && ne(theirs, ours) {

                                                d.push(format!("Clone {} #{go} {k}: game {theirs:?} ours {ours:?}", r.name));
                                            }
                                        }
                                    }
                                }
                            }
                            // gapC r8: the self's ParticleSystem fields after the call (FnResult " psf="): isEmitting against ours' system of
                            // that field; a field ours has no system for is a difference only when the game's changed (isPlaying: not modelled).
                            if let (Some(post), Some(sid)) = (s.of("FnResult").next().and_then(|x| x.get("psf")), pi.of("FnCase").next().and_then(|c| c.get("self")).and_then(|v| v.parse::<i32>().ok())) {
                                let pre: std::collections::HashMap<String, String> = pi.of("FnCase").next().and_then(|c| c.get("pspre")).map(ps_fields).unwrap_or_default().into_iter().collect();
                                for (f, v) in ps_fields(post) {
                                    let Some(theirs) = v.chars().next().filter(|x| *x != '-') else { continue };
                                    match ps_field(&g, sid, &f) {
                                        Some(i) => {
                                            let ours = if crate::particles::emitting(&g, i) { '1' } else { '0' };
                                            if ours != theirs { d.push(format!("ParticleSystem {f} of #{sid} isEmitting: game {theirs} ours {ours}")) }
                                            // gapC r11: isPlaying and emission.enabled (r11 recordings). isPlaying is left out after a PI that was
                                            // stopping with live particles ("01" with emission on: ours loads no particle).
                                            let (b, pb) = (v.as_bytes(), pre.get(&f).map(|p| p.as_bytes().to_vec()).unwrap_or_default());
                                            // gapC r12: and after a Stop of a system the PI had playing ("11x" -> Stop -> "011": Decimator/Generator
                                            // UpdateActivation): it keeps playing while its particles live (checker limit: ours loads no particle).
                                            let stopped = g.ps_calls.iter().rev().find(|(k, _)| *k == i).is_some_and(|(_, c)| matches!(c, crate::game::PsCall::Stop(_)));
                                            if b.len() >= 3 && pb.get(0..3) != Some(b"011") && !(stopped && pb.get(1) == Some(&b'1')) {
                                                let ours = crate::particles::playing(&g, i);
                                                if (b[1] == b'1') != ours { d.push(format!("ParticleSystem {f} of #{sid} isPlaying: game {} ours {}", b[1] as char, ours as u8)) }
                                                let ours = crate::particles::emission_on(&g, i);
                                                if (b[2] == b'1') != ours { d.push(format!("ParticleSystem {f} of #{sid} emission.enabled: game {} ours {}", b[2] as char, ours as u8)) }
                                            }
                                        }
                                        // gapC r16: FirearmBehaviour.bulletCasing's Play is ours' casing effect (firearm.rs Shoot): emitting
                                        // when the call spawned it (checker limit: a burst, isPlaying not compared).
                                        None if f == "bulletCasing" && let Some(fa) = g.firearms.iter().find(|x| x.id == sid) => {
                                            let ours = if fa.casing_prefab.is_some_and(|p| g.effect_log.iter().any(|e| e.prefab == p)) { '1' } else { '0' };
                                            if ours != theirs { d.push(format!("ParticleSystem {f} of #{sid} isEmitting: game {theirs} ours {ours}")) }
                                        }
                                        // gapC r18: a BleedingParticleBehaviour's own system: emission.enabled against ours' particle's; isEmitting as a
                                        // playing system's emission (checker limit: isPlaying not modelled, the system plays from Awake on, the PI's is used).
                                        None if f == "BloodParticleSystem" && let Some(b) = bleeding_of(&g, &pi, sid) => {
                                            let en = g.bleeding[b].emission_enabled;
                                            let playing = pre.get(&f).and_then(|p| p.as_bytes().get(1).copied()) == Some(b'1');
                                            let ours = if en && playing { '1' } else { '0' };
                                            if ours != theirs { d.push(format!("ParticleSystem {f} of #{sid} isEmitting: game {theirs} ours {ours}")) }
                                            if let Some(&e) = v.as_bytes().get(2) && (e == b'1') != en { d.push(format!("ParticleSystem {f} of #{sid} emission.enabled: game {} ours {}", e as char, en as u8)) }
                                        }
                                        // fnrec r2: a power tool's effect: isEmitting against ours' playing (fnrec r3; checker limit: isPlaying
                                        // not compared, it stays 1 after Stop while particles live and ours loads no particle).
                                        None if f == "effect" && let Some(k) = pi.of("FnCase").next().and_then(|c| c.get("selfty")).and_then(crate::game::power::PowerKind::of_tool)
                                            && let Some(p) = g.powers.iter().find(|p| p.kind == k) => {
                                            if (theirs == '1') != p.playing { d.push(format!("ParticleSystem effect of #{sid} isEmitting: game {theirs} ours {}", p.playing as u8)) }
                                        }
                                        None if pre.get(&f).and_then(|p| p.chars().next()) != Some(theirs) => d.push(format!("ParticleSystem {f} of #{sid} isEmitting: game {theirs}, ours has no system")),
                                        None => {}
                                    }
                                }
                            }
                            // gapC r11: the call's PlayOneShot calls in order (FnResult " shots=<source>:<clip>:<volumeScale bits>;..."; a null
                            // clip plays nothing and ours never logs one), against ours' shot_log. Recordings since recver 3.
                            if ver >= 3 {
                                let theirs: Vec<&str> = s.of("FnResult").next().and_then(|x| x.get("shots")).map_or(Vec::new(), |v| v.split(';').filter(|x| x.split(':').nth(1) != Some("null")).collect());
                                let ours = &g.audio.shot_log;
                                // fc41 r19: a scene AudioSource (UISoundBehaviour's, DistantSoundBehaviour's) has a positive level1 id in the
                                // game and a made-up one below i32::MIN / 2 in ours (scene.rs scene_audio_source): such a shot compares
                                // its clip and volume only. Limit: which scene source played is not checked.
                                let scene = |o: &str| o.split(':').next().and_then(|x| x.parse::<i32>().ok()).is_some_and(|x| x < i32::MIN / 2);
                                let same = |t: &str, o: &str| if scene(o) { t.split_once(':').map(|x| x.1) == o.split_once(':').map(|x| x.1) && !t.starts_with('-') } else { t == o };
                                if theirs.len() != ours.len() || theirs.iter().zip(ours).any(|(t, o)| !same(t, o)) {
                                    d.push(format!("PlayOneShot calls: game [{}] ours [{}]", theirs.join(";"), ours.join(";")));
                                }
                            }
                            // gapC r20: the call's own Object.Destroy calls (FnResult " destroys=<g|c><instance id>:<t bits>", recver 4), against
                            // what the call queued in ours (a GameObject's queue entry names one of its components). Not compared when the call
                            // queued a joint destroy (those queues hold no instance id).
                            if ver >= 4 && let Some(fr) = s.of("FnResult").next() {
                                let theirs: std::collections::BTreeSet<String> = fr.get("destroys").map_or(Default::default(), |v| v.split(';').map(str::to_string).collect());
                                let now = g.time.fixedTimeAsDouble.min(g.time.timeAsDouble);
                                let t = |at: f64| format!("{:08x}", ((at - now) as f32).max(0.0).to_bits());
                                let go = |g: &Game, id: i32| g.unsynced.iter().chain(&g.retained).flat_map(|i| &i.objects).find(|o| o.components.contains(&id)).map_or(id, |o| o.game_object);
                                let (pc, po, pj, pa, _) = &ours_pre_destroys;
                                let mut ours: std::collections::BTreeSet<String> = g.component_destroys.iter().filter(|e| !pc.contains(e)).map(|&(at, id)| format!("c{id}:{}", t(at))).collect();
                                // gapA r22: DragTool's HingeJoint2D (Object.Destroy(dragJoint), queued in tool.rs, not component_destroys).
                                if let Some(tl) = g.tool.as_ref() {
                                    ours.extend(tl.destroying.iter().chain(&tl.dragJoint).filter_map(|j| j.destroy_at.map(|at| (j.id, at))).filter(|&(id, at)| !drag_pre.contains(&(id, Some(at)))).map(|(id, at)| format!("c{id}:{}", t(at))));
                                }
                                // gapB r25: a gore string's SpringJoint2D (GoreStringBehaviour.DestroyJoint's Object.Destroy(Joint), queued as gore.destroy_at).
                                ours.extend(g.limbs.iter().filter(|l| l.gore.spring_id != 0).filter_map(|l| l.gore.destroy_at.map(|at| (l.gore.spring_id, at))).filter(|&(id, at)| !gore_pre.contains(&(id, Some(at)))).map(|(id, at)| format!("c{id}:{}", t(at))));
                                // v52h round 2: a limb's Object.Destroy(Joint) (limb_destroy_joint: hinges[h].destroy_at).
                                ours.extend(limb_hinge_pre.iter().filter_map(|&(h, id, pre)| g.hinges[h].destroy_at.filter(|&at| pre != Some(at)).map(|at| format!("c{id}:{}", t(at)))));
                                ours.extend(g.object_destroys.iter().filter(|e| !po.contains(e)).map(|&(at, id)| format!("g{}:{}", go(&g, id), t(at))));
                                // fc41 r6: a " balpop=1" Balloon.Shot case: FnTiny.R4Of runs WaitAFrame on to Pop after the recorder's
                                // destroys capture closed, so the game's list misses Pop's Destroy (a checker limit; the rest compares).
                                let balpop = name == "BalloonBehaviour.Shot" && pi.of("FnCase").next().and_then(|r| r.get("balpop")) == Some("1");
                                if theirs != ours && !balpop && g.joint_destroys.len() == *pj && g.attachment_joint_destroys.len() == *pa {
                                    d.push(format!("Object.Destroy calls: game [{}] ours [{}]", theirs.into_iter().collect::<Vec<_>>().join(";"), ours.into_iter().collect::<Vec<_>>().join(";")));
                                }
                                // Tritium.OnEnterContainer/OnExitContainer: the container's GlowInTheDarkBehaviour after the call (" glow=", 0: none).
                                if let Some(glow) = fr.get("glow").and_then(|v| v.parse::<i32>().ok()) {
                                    let live = |id: i32| g.glows.iter().any(|x| x.id == id && !g.destroyed_components.contains(&id));
                                    if (glow != 0 && !live(glow)) || (glow == 0 && g.glows.len() != ours_pre_destroys.4) {
                                        d.push(format!("GlowInTheDarkBehaviour after the call: game #{glow}, ours [{}]", g.glows.iter().map(|x| x.id.to_string()).collect::<Vec<_>>().join(";")));
                                    }
                                }
                            }
                            // gapC r9: the static fields after the call (FnResult " sf="), against ours' (static_field).
                            if let Some(post) = s.of("FnResult").next().and_then(|x| x.get("sf")) {
                                for (f, theirs) in ps_fields(post) {
                                    if let Some(ours) = static_field(&mut g, &f).map(|x| x.show()) && ours != theirs {
                                        d.push(format!("static {f}: game {theirs} ours {ours}"));
                                    }
                                }
                            }
                            // batch2 r9: compare() leaves the FireLoopChunks out: each recorded chunk's Dirty, activeSelf, AudioSource and its
                            // volume, and isPlaying (`playing=`, since r9).
                            // gapC r16: and FireSoundEmitter's (FixedUpdate's FuzzySetVolumeAt sets a chunk's volume).
                            if name.starts_with("FireLoopChunkBehaviour.") || name.starts_with("FireSoundEmitter.") {
                                for r in s.of("FireLoopChunk") {
                                    let Some(k) = g.fire_loop_chunks.values().find(|k| k.id == r.id) else { d.push(format!("FireLoopChunk #{}: not ours", r.id)); continue };
                                    let src = k.source.map(|i| (i, g.audio.get(i)));
                                    let ours = [("dirty", bit(k.dirty)), ("active", src.map_or(String::new(), |(_, a)| bit(a.active))),
                                        ("source", src.map_or("null".into(), |(i, a)| format!("{i},{}", hex(a.volume)))), ("playing", src.map_or("0".into(), |(_, a)| bit(a.is_playing())))];
                                    for (key, ours) in ours {
                                        // A chunk without an AudioSource: the rewrite keeps no activeSelf for it.
                                        let Some(theirs) = r.get(key).filter(|_| key != "active" || src.is_some()) else { continue };
                                        if theirs != ours { d.push(format!("FireLoopChunk #{} {key}: game {theirs} ours {ours}", r.id)) }
                                    }
                                }
                            }
                            // batch2 r10: EmitShrapnel only starts EmitRoutine: the pending routines (`ShrapnelRoutines pending=`, oldest
                            // first, "x,y:amount:state"; state 1 waits for the end of frame, 2 for the FixedUpdate) against ours.
                            if name == "GlobalShrapnelEmitter.EmitShrapnel" && let Some(r) = s.of("ShrapnelRoutines").next() {
                                use crate::game::destroyable::Step;
                                let mut ours: Vec<(u64, String)> = g.end_of_frame.iter().chain(&g.fixed_update_waits).filter(|c| c.owner == crate::game::ballistics::SHRAPNEL_OWNER).filter_map(|c| match c.step {
                                    Step::ShrapnelEndOfFrame(p, n) => Some((c.seq, format!("{},{}:{n}:1", hex(p[0]), hex(p[1])))),
                                    Step::ShrapnelEmit(p, n) => Some((c.seq, format!("{},{}:{n}:2", hex(p[0]), hex(p[1])))),
                                    _ => None,
                                }).collect();
                                ours.sort_by_key(|x| x.0);
                                let ours = if ours.is_empty() { "-".to_string() } else { ours.into_iter().map(|x| x.1).collect::<Vec<_>>().join(";") };
                                let theirs = r.get("pending").unwrap_or("?");
                                if theirs != ours { d.push(format!("ShrapnelRoutines pending: game {theirs} ours {ours}")) }
                            }
                            // batch2 r10: DisintegrationCounterBehaviour.children (a link item_fields leaves out), as ids in order.
                            if name.starts_with("DisintegrationCounterBehaviour.") {
                                for r in s.of("ItemScript").filter(|r| r.get("class") == Some("DisintegrationCounterBehaviour")) {
                                    let (Some(x), Some(v)) = (g.disintegration_counters.iter().find(|x| x.id == r.id), r.get("children")) else { continue };
                                    let ours: Vec<i32> = x.children.iter().map(|&p| g.physicals[p].id).collect();
                                    if ref_ids(v) != ours { d.push(format!("DisintegrationCounter #{} children: game {v} ours {ours:?}", r.id)) }
                                }
                                // batch2 r12: the Human's own counter: DisintegrationCount, children, and each physical's subscriptions.
                                for r in s.of("HumanDisintegrationCounter") {
                                    let Some(person) = human_counter_person(&mut g, r.id) else { continue };
                                    let children: Vec<i32> = human_counter_children(&g, person).iter().map(|&p| g.physicals[p].id).collect();
                                    let first: Vec<usize> = g.people[person].Limbs.iter().map(|&l| g.limbs[l].PhysicalBehaviour).chain(g.people[person].pseudo_children.iter().copied()).collect();
                                    let mut ours: Vec<(i32, usize)> = Vec::new();
                                    for p in first.iter().chain(&g.people[person].counter_subs) {
                                        let id = g.physicals[*p].id;
                                        match ours.iter_mut().find(|x| x.0 == id) {
                                            Some(x) => x.1 += 1,
                                            None => ours.push((id, 1)),
                                        }
                                    }
                                    let mut game = human_counter_subs(r.get("subs").unwrap_or(""));
                                    (ours.sort(), game.sort());
                                    let count = g.people[person].DisintegrationCount.to_string();
                                    if r.get("DisintegrationCount") != Some(count.as_str()) { d.push(format!("HumanDisintegrationCounter DisintegrationCount: game {:?} ours {count}", r.get("DisintegrationCount"))) }
                                    if list_ids(r.get("children").unwrap_or("")) != children { d.push(format!("HumanDisintegrationCounter children: game {:?} ours {children:?}", r.get("children"))) }
                                    if game != ours { d.push(format!("HumanDisintegrationCounter subs: game {game:?} ours {ours:?}")) }
                                }
                            }
                            // batch4 r9: TeslaCoil OnImmobilityCapture/Release: whCapturers (a link compare() leaves out), as id sets.
                            if name.starts_with("TeslaCoilBehaviour.OnImmobility") {
                                for r in s.of("ItemScript").filter(|r| r.get("class") == Some("TeslaCoilBehaviour")) {
                                    let (Some(coil), Some(theirs)) = (g.tesla_coils.iter().find(|x| x.id == r.id), r.get("whCapturers")) else { continue };
                                    let mut ours: Vec<i32> = coil.whCapturers.iter().map(|&f| g.immobility_fields[f].id).collect();
                                    let mut game = ref_ids(theirs);
                                    (ours.sort(), game.sort());
                                    if ours != game { d.push(format!("TeslaCoil #{} whCapturers: game {game:?} ours {ours:?}", r.id)) }
                                }
                            }
                            // batch4 r3: compare() leaves damagePoints out; OnAfterDeserialise's only effect is that array.
                            // gapB r24: Update's RegenerateWounds and stab wounds write the self's array only: that one compared.
                            let me = pi.of("FnCase").next().and_then(|c| c.get("self")).and_then(|v| v.parse::<i32>().ok());
                            if name == "SkinMaterialHandler.OnAfterDeserialise" || name == "SkinMaterialHandler.Update" {
                                for (k, r) in g.skins.iter().zip(s.of("SkinMaterialHandler")).filter(|(_, r)| name.ends_with("OnAfterDeserialise") || Some(r.id) == me) {
                                    let ours: Vec<String> = k.damagePoints.iter().map(|p| vec(*p)).collect();
                                    let theirs = r.get("damagePoints").unwrap_or("");
                                    if format!("[{}]", ours.join(";")) != theirs { d.push(format!("Skin {} damagePoints: game {} entries ours {}", r.name, theirs.split(';').count(), ours.len())) }
                                }
                            }
                            d
                        })) {
                            Ok(more) => d.extend(more),
                            Err(_) => d.push("panic in the comparison".into()),
                        }
                        d
                    }
                };
                #[cfg(fncov)]
                if diffs.is_empty() {
                    cov::passed(&name);
                }
                let t = tally.entry(name).or_default();
                if diffs.is_empty() {
                    t.pass += 1;
                } else if diffs.len() == 1 && diffs[0].starts_with("too old: ") {
                    t.old += 1;
                    t.first.get_or_insert_with(|| diffs[0].clone());
                } else {
                    t.fail += 1;
                    if std::env::var("PPG_FNCHECK_ALL").is_ok() { eprintln!("FAILCASE {} target #{} args={}: {}", pi.of("FnCase").next().unwrap().name, r.id, pi.of("FnCase").next().unwrap().get("args").unwrap_or(""), diffs.iter().take(6).cloned().collect::<Vec<_>>().join(" | ")); }
                    t.first.get_or_insert_with(|| format!("target #{} args={}: {}", r.id, pi.of("FnCase").next().unwrap().get("args").unwrap_or(""), diffs.iter().take(6).cloned().collect::<Vec<_>>().join(" | ")));
                }
            }
            _ => {}
        }
    }
    // gapA r18: header() already holds every non-snapshot line; chaining snaps.header (the same lines, read by the loop
    // above) counted each fnpure and fnctor row twice.
    for line in header.iter() {
        if let Some(rest) = line.strip_prefix("fnpure ") {
            let (name, rest) = rest.split_once(' ').unwrap_or((rest, ""));
            if !in_shard(name) {
                continue;
            }
            let args_text = rest.split(" ret=").next().unwrap().strip_prefix("args=").unwrap_or("");
            let args: Vec<&str> = split_args(args_text);
            let theirs = rest.split(" ret=").nth(1).unwrap_or("");
            let t = tally.entry(name.to_string()).or_default();
            // A generated free function: its args in a Case over an empty snapshot (no self).
            let pure_gen = || {
                let (_, call) = PURE_DLL.iter().chain(PURE_DLL2.iter()).chain(PURE_MODS.iter()).chain(PURE_LEGACY.iter()).chain(PURE_GEN.iter()).find(|(n, _)| *n == name)?;
                let s = Snapshot { pt: String::new(), snap: Vec::new(), records: Vec::new() };
                let case = Case { s: &s, id: 0, sid: 0, args: args.clone(), liq: None, ver };
                Some(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call(&case))).unwrap_or_else(|_| Err("panic".into())).unwrap_or_else(|e| format!("error: {e}")))
            };
            // gapA r18: the mutant is the PI one's "skip the call" (skipped_call), not "": "" only matched a void row, so a
            // non-void row's mutant died whatever the cases held.
            let ours = pure(name, &args).or_else(pure_gen);
            match if mutated(name) { ours.map(|o| skipped_call(&o, &args)) } else { ours } {
                // A void method: the recorder's Call writes "void" (strings are quoted), ours "" (VOID), as the PI path's FnResult
                // compare; the DLL pure entries that answer "void" themselves still match as is.
                Some(ours) if ours == theirs || (theirs == "void" && ours.is_empty()) || after_only(&ours, theirs, &args) => t.pass += 1,
                ours => {
                    t.fail += 1;
                    // PPG_FNCHECK_ALL: every failing row, in the FAIL line's form (tools/fnmods/diff.py reads it).
                    if std::env::var("PPG_FNCHECK_ALL").is_ok() { eprintln!("FAIL {name} row: args={args_text}: game {theirs} ours {ours:?}"); }
                    t.first.get_or_insert_with(|| format!("args={args_text}: game {theirs} ours {ours:?}"));
                }
            }
        } else if let Some(rest) = line.strip_prefix("fnmissing ") {
            let (name, why) = rest.split_once(' ').unwrap_or((rest, "the game has no such method"));
            if !in_shard(name) {
                continue;
            }
            tally.entry(name.to_string()).or_default().first.get_or_insert_with(|| format!("not called: {why}"));
        }
    }
    // Constructors (fnctor.rs): the fields of an object as the game makes it against the rewrite struct's Default.
    let ctor: Vec<&str> = header.iter().filter_map(|l| l.strip_prefix("fnctor ")).collect();
    for (name, r) in super::fnctor::check(&ctor, in_shard) {
        let t = tally.entry(name).or_default();
        match r {
            Ok(n) => t.pass += n,
            Err(e) => {
                t.fail += 1;
                t.first.get_or_insert(e);
            }
        }
    }
    #[cfg(fncov)]
    cov::write();
    let (mut failed, mut old_n) = (0, 0);
    for (name, t) in &tally {
        if t.fail == 0 && t.pass > 0 {
            println!("PASS {name} {}{}", t.pass, if t.old > 0 { format!(" ({} too old)", t.old) } else { String::new() });
        } else if t.fail == 0 && t.old > 0 {
            old_n += 1;
            println!("OLD {name} {}: {}", t.old, t.first.as_deref().unwrap_or(""));
        } else {
            failed += 1;
            println!("FAIL {name} {}/{}: {}", t.fail, t.pass + t.fail, t.first.as_deref().unwrap_or(""));
        }
    }
    println!("fn-check: {} of {} functions pass{}", tally.len() - failed - old_n, tally.len(), if old_n > 0 { format!(" ({old_n} recorded too old)") } else { String::new() });
    if failed == 0 { Ok(()) } else { Err(format!("{failed} functions differ")) }
}

/// gapA r19: a pure row and ours that differ only in the args-after part one of the two leaves out: an objects-mode
/// DllCheck row records no ` after=` (ours' seeded-form twin adds it), and a seeded-mode row with no args writes an empty
/// ` after=` (ours' objects-form twin writes none), or one whose after= is the args as passed (a by-value arg: the
/// objects-form twin writes none). Only what the row records is compared.
fn after_only(ours: &str, theirs: &str, args: &[&str]) -> bool {
    match (ours.split_once(" after="), theirs.split_once(" after=")) {
        (Some((o, _)), None) => o == theirs,
        (None, Some((t, a))) => t == ours && (a.is_empty() || a == args.get(1..).unwrap_or(&[]).join(";")),
        _ => false,
    }
}

#[test]
fn after_only_forms() {
    assert!(after_only("void set(X=1) after=2", "void set(X=1)", &[]));
    assert!(after_only("void set(X=1)", "void set(X=1) after=", &[]));
    assert!(after_only("void set(X=1)", "void set(X=1) after=c3340000", &["@self", "c3340000"]));
    assert!(!after_only("void set(X=1)", "void set(X=1) after=00000000", &["@self", "c3340000"]));
}

/// gapA r19: the items a `;` list gained at its end (None: not a pure append, or no list before).
fn menu_appended<'a>(pre: Option<&str>, post: &'a str) -> Option<Vec<&'a str>> {
    let pre: Vec<&str> = pre?.split(';').filter(|x| !x.is_empty()).collect();
    let post: Vec<&str> = post.split(';').filter(|x| !x.is_empty()).collect();
    (post.len() >= pre.len() && post.iter().zip(&pre).all(|(a, b)| a == b)).then(|| post[pre.len()..].to_vec())
}

#[test]
fn menu_appended_forms() {
    assert_eq!(menu_appended(Some("a;b"), "a;b;c"), Some(vec!["c"]));
    assert_eq!(menu_appended(Some(""), "c"), Some(vec!["c"]));
    assert_eq!(menu_appended(Some("a;b"), "b"), None);
    assert_eq!(menu_appended(None, "b"), None);
}

/// batch4 r9: the ids of a recorded reference list (`[@a#1;@b#2]`).
fn ref_ids(v: &str) -> Vec<i32> {
    v.trim_matches(|x| x == '[' || x == ']').split(";").filter(|x| !x.is_empty()).filter_map(reference).collect()
}

/// batch4 r9: a TeslaCoil Immobility case: the coil (by the case's self), its whCapturers from the PI record, the sender's
/// `immobility_fields` index and one of the coil's colliders (the SendMessage route the rewrite takes).
fn tesla_case(g: &mut Game, c: &Case) -> Result<(usize, usize, i32), String> {
    let i = g.tesla_coils.iter().position(|x| x.id == c.sid).ok_or("no TeslaCoilBehaviour")?;
    let r = c.s.of("ItemScript").find(|r| r.id == c.sid).ok_or("no TeslaCoil record")?;
    let field = |g: &Game, id: i32| g.immobility_fields.iter().position(|x| x.id == id).ok_or(format!("no ImmobilityFieldBehaviour #{id}"));
    let set = ref_ids(r.req("whCapturers")?).into_iter().map(|id| field(g, id)).collect::<Result<Vec<_>, _>>()?;
    g.tesla_coils[i].whCapturers = set;
    let f = field(g, reference(c.args.first().copied().ok_or("no sender")?).ok_or("null sender")?)?;
    let col = *g.tesla_coils[i].colliders.first().ok_or("a coil without colliders")?;
    Ok((i, f, col))
}

/// gapB r27: FnTiny's gb27 state, "rh:<IsHovering>" (the scene's ResizeHandles) or "ssa:<enabled>,<isPlaying>,<volume bits>"
/// (the PhysicalBehaviour `sid`'s slidingAudioSource; "ssa:none" without one).
fn gb27_state(g: &Game, sid: i32, rh: bool) -> String {
    if rh {
        return format!("rh:{}", g.resize.IsHovering as u8);
    }
    match g.physicals.iter().find(|p| p.id == sid).map(|p| p.ids.slidingAudioSource).filter(|id| g.audio.sources.contains_key(id)) {
        Some(id) => {
            let s = g.audio.get(id);
            format!("ssa:{},{},{:08x}", s.enabled as u8, s.is_playing() as u8, s.volume.to_bits())
        }
        None => "ssa:none".into(),
    }
}

/// gapB r27: a gb27pre= state into ours before the call (gb27_state's form).
fn gb27_load(g: &mut Game, sid: i32, v: &str) -> Result<(), String> {
    if let Some(h) = v.strip_prefix("rh:") {
        g.resize.IsHovering = h == "1";
        return Ok(());
    }
    let s = v.strip_prefix("ssa:").ok_or_else(|| format!("gb27pre: {v}"))?;
    if s == "none" {
        return Ok(());
    }
    let [en, pl, vol] = s.split(',').collect::<Vec<_>>()[..] else { return Err(format!("gb27pre: {v}")) };
    let id = g.physicals.iter().find(|p| p.id == sid).map(|p| p.ids.slidingAudioSource).filter(|id| g.audio.sources.contains_key(id)).ok_or_else(|| {
        let p = g.physicals.iter().find(|p| p.id == sid);
        format!("gb27pre: the self has no slidingAudioSource in ours (physical {}, PlaySliderSound {:?}, id {:?})", p.is_some(), p.map(|p| p.PlaySliderSound), p.map(|p| p.ids.slidingAudioSource))
    })?;
    g.audio.set_enabled(id, en == "1");
    if pl == "1" {
        if !g.audio.is_playing(id) {
            g.audio.play(id);
        }
    } else {
        g.audio.stop(id);
    }
    g.audio.get_mut(id).set_volume(f32::from_bits(u32::from_str_radix(vol, 16).map_err(|_| "gb27pre volume")?));
    Ok(())
}
