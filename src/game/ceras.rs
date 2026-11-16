//! The Ceras binary format of a contraption (`.jaap`, ContraptionSerialiser.cerasSerializer): the object graph of `Contraption` / `ObjectState` /
//! `MonoBehaviourPrototype`, read and written byte for byte as Ceras 4.0 does with `HappyTypeBinder` and `CerasUnityFormatters` and no version tolerance.
//!
//! Wire facts (Ceras `SerializerBinary`, `ReferenceFormatter`, `TypeFormatter`, `DynamicFormatter`, `ReinterpretFormatter`, `CollectionFormatter`):
//! * varints are LEB128; signed ones zigzag (`WriteInt32`: bool, int, enum), `*Bias` ones are plain varints of `value + bias`;
//! * a string is `bias(1)` of its UTF-8 byte count (null = -1) and the bytes; it is never reference-cached;
//! * a reference-typed value is `bias(5)` of -1 null, -2 new value of a different type (then the type), -3 new value of the static type, -4 external, -5 inline
//!   Type, or the id (0, 1, ...) of an earlier object; every non-null object takes the next id in the order it is first written, before its members;
//! * a `Type` is `bias(3)` of -1 null, -2 generic, -3 new (then its name as a string) or the id of an earlier one; no object wraps it;
//! * members of a class or struct are written in `SchemaMemberComparer` order (member type name, then member name); a blittable struct (`IsBlittableType`,
//!   unmanaged: Vector2/3/4, Color, Bounds, Guid, `TransformPrototype`, `SharpAxis`, ...) and an array of one are the raw memory instead;
//! * `Dictionary` and other collections: plain varint count, then the entries (key, value); an array of non-blittable items: `bias(1)` count; an array of
//!   blittable items (or of a primitive) is the plain varint count and the raw items.
//!
//! Boxed `object` values carry their type, so the payload is decoded from the type's name (`kind`).
use std::collections::HashMap;

/// How a boxed value of one .NET type is written.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Bool,
    F32,
    F64,
    I32,
    U32,
    U16,
    U8,
    Str,
    /// An enum with an Int32 underlying type (zigzag varint).
    EnumI32,
    /// An enum with a Byte underlying type.
    EnumU8,
    /// A blittable struct: its memory.
    Raw(usize),
    /// An array of blittable items of this size (plain varint count, then the memory).
    RawArray(usize),
    /// `BloodContainer.SerialisableDistribution[]`: `bias(1)` count, each (Amount: float, LiquidID: string).
    Distributions,
}

/// HappyTypeBinder.BlockedNamespaces
pub(crate) const BLOCKED_NAMESPACES: [&str; 12] = ["System.Diagnostics", "System.IO", "System.Net", "System.Reflection", "System.Runtime", "System.Security", "System.Threading", "System.Activator", "System.AppDomain", "System.Management", "Microsoft", "System.Linq.Expressions"];

/// HappyTypeBinder.StripAssemblyInfo: the name up to its first comma outside <> and [] brackets.
pub(crate) fn strip_assembly_info(name: &str) -> &str {
    let mut depth = 0;
    for (i, c) in name.char_indices() {
        match c {
            '<' | '[' => depth += 1,
            '>' | ']' => depth -= 1,
            ',' if depth == 0 => return &name[..i],
            _ => {}
        }
    }
    name
}

/// HappyTypeBinder.ReportMissingType: a Logger line (the Logger is empty), then null.
pub fn happy_type_binder_report_missing_type(_name: Option<&str>) -> Option<String> {
    None
}

/// HappyTypeBinder.GetTypeFromBase: null for a blank name; else the name without its assembly part, looked up in
/// HappyTypeBinder.Assemblies (the game, UnityEngine.Physics2DModule, UnityEngine.CoreModule, mscorlib; a type the reader
/// has no form for fails at its value, `kind`): a type none of them defines (UnityEngine.AudioSource lives in
/// UnityEngine.AudioModule) is ReportMissingType, null; then HappyTypeBinder.CheckType (which also refuses a delegate:
/// `typeof(Delegate).IsAssignableFrom(type)`).
pub fn happy_type_binder_get_type_from_base(base: Option<&str>) -> Option<String> {
    let base = base.filter(|b| !b.trim().is_empty())?;
    let name = strip_assembly_info(base);
    match binder_assemblies_define(name) {
        None => happy_type_binder_report_missing_type(Some(base)),
        Some(true) => happy_type_binder_report_missing_type(Some(name)),
        Some(false) => happy_type_binder_check_type(name).map(str::to_string),
    }
}

/// `Assembly.GetType(name) != null` for one of HappyTypeBinder.Assemblies: a top-level or nested (`Outer+Inner`) TypeDef,
/// or an array of one: Some(whether it is a delegate type: System.Delegate, System.MulticastDelegate or a type extending
/// MulticastDelegate; an array never is). The assemblies are read once from the installed game; without it every name is
/// taken as defined and not a delegate.
fn binder_assemblies_define(name: &str) -> Option<bool> {
    static ASSEMBLIES: std::sync::OnceLock<Option<Vec<crate::assets::clr::Assembly>>> = std::sync::OnceLock::new();
    let Some(assemblies) = ASSEMBLIES.get_or_init(|| {
        let dir = crate::game_assets::find_game(None).ok()?.join("People Playground_Data/Managed");
        ["Assembly-CSharp", "UnityEngine.Physics2DModule", "UnityEngine.CoreModule", "mscorlib"].iter().map(|a| crate::assets::clr::Assembly::open(&dir.join(format!("{a}.dll"))).ok()).collect()
    }) else {
        return Some(false);
    };
    let mut element = name;
    while let Some(e) = element.strip_suffix("[]") {
        element = e;
    }
    let array = element.len() != name.len();
    let (namespace, short) = match element.split('+').next().and_then(|outer| outer.rfind('.')) {
        Some(dot) => (&element[..dot], element[dot + 1..].replace('+', "/")),
        None => ("", element.replace('+', "/")),
    };
    let is_delegate = |a: &crate::assets::clr::Assembly, t: u32| {
        let sys = |n: &str| namespace == "System" && short == n;
        sys("Delegate") || sys("MulticastDelegate") || a.type_name(a.coded(crate::assets::clr::TYPE_DEF, t, 3)).is_some_and(|(ns, n)| ns == "System" && n == "MulticastDelegate")
    };
    assemblies.iter().find_map(|a| a.type_def(namespace, &short).map(|t| !array && is_delegate(a, t)))
}

/// HappyTypeBinder.GetTypeFromBaseAndArguments: the definition through GetTypeFromBase, null without arguments; an argument
/// that is null or fails CheckType is ReportMissingType (null); Type.MakeGenericType throwing for the wrong number of
/// arguments is a LogWarning and null. A constructed type's name here is its FullName without the assembly parts:
/// `Definition`N[[A],[B]]`.
pub fn happy_type_binder_get_type_from_base_and_arguments(base: &str, arguments: Option<&[Option<String>]>) -> Option<String> {
    let definition = happy_type_binder_get_type_from_base(Some(base))?;
    let arguments = arguments?;
    let mut names = Vec::with_capacity(arguments.len());
    for a in arguments {
        names.push(happy_type_binder_check_type(a.as_deref()?)?);
    }
    let arity = definition.rsplit_once('`').and_then(|(_, n)| n.parse::<usize>().ok())?;
    if arity != names.len() {
        return None;
    }
    Some(format!("{definition}[{}]", names.iter().map(|n| format!("[{n}]")).collect::<Vec<_>>().join(",")))
}

/// A constructed generic type's definition and arguments (`happy_type_binder_get_type_from_base_and_arguments`' names).
fn generic_parts(name: &str) -> Option<(&str, Vec<&str>)> {
    let open = name.find("[[")?;
    let inner = name[open + 1..].strip_suffix(']')?;
    let (mut depth, mut start, mut parts) = (0, 0, Vec::new());
    for (i, c) in inner.char_indices() {
        match c {
            '[' => {
                if depth == 0 {
                    start = i + 1;
                }
                depth += 1;
            }
            ']' => {
                depth -= 1;
                if depth == 0 {
                    parts.push(&inner[start..i]);
                }
            }
            _ => {}
        }
    }
    Some((&name[..open], parts))
}

/// HappyTypeBinder.CheckType: null for a pointer, by-ref or delegate type (`kind` has none) and for one whose FullName starts
/// with a BlockedNamespaces entry (ordinal).
pub(crate) fn happy_type_binder_check_type(full_name: &str) -> Option<&str> {
    if full_name.ends_with('*') || full_name.ends_with('&') || BLOCKED_NAMESPACES.iter().any(|b| full_name.starts_with(b)) {
        return None;
    }
    Some(full_name)
}

/// The wire form of a .NET type by its `Type.FullName` (what `HappyTypeBinder.GetBaseName` writes), None for a type not rewritten.
pub fn kind(ty: &str) -> Option<Kind> {
    if let Some(item) = ty.strip_suffix("[]") {
        if item == "BloodContainer+SerialisableDistribution" {
            return Some(Kind::Distributions);
        }
        return match item {
            "System.Single" | "System.Int32" | "System.UInt32" => Some(Kind::RawArray(4)),
            // Marshal.SizeOf(bool) is 4: the blit copies four bytes per item (the three after the first can be stray memory, so the bytes are kept as read)
            "System.Boolean" => Some(Kind::RawArray(4)),
            "System.Byte" => Some(Kind::RawArray(1)),
            "System.UInt16" => Some(Kind::RawArray(2)),
            "System.Double" => Some(Kind::RawArray(8)),
            _ => raw_size(item).map(Kind::RawArray),
        };
    }
    Some(match ty {
        "System.Boolean" => Kind::Bool,
        "System.Single" => Kind::F32,
        "System.Double" => Kind::F64,
        "System.Int32" => Kind::I32,
        "System.UInt32" => Kind::U32,
        "System.UInt16" => Kind::U16,
        "System.Byte" => Kind::U8,
        "System.String" => Kind::Str,
        "UnityEngine.HideFlags" | "UnityEngine.KeyCode" | "UnityEngine.ForceMode2D" | "UnityEngine.RectTransform+Axis" | "Decal" | "DecalInformation+DecalIndex" | "DistantSoundBehaviour+SoundType"
        | "BloodContainer+PressureDirection" | "FanBehaviour+FanMode" | "JetEngineBehaviour+State" | "SentryTurretAIState" | "BloodTankBehaviour+TankMode" => Kind::EnumI32,
        "Activations.Channel" | "LimbBehaviour+ShatterFlags" => Kind::EnumU8,
        // An enum of a saved member: EnumFormatter writes it as its underlying integer.
        _ => match super::save_members::ENUM_TYPES.iter().find(|(e, _)| *e == ty).map(|(_, u)| *u) {
            Some("System.Int32") => Kind::EnumI32,
            Some("System.Byte") => Kind::EnumU8,
            // save_members::ENUM_TYPES (generated from the game's enums) has only Int32 and Byte ones.
            Some(u) => unreachable!("Ceras: an enum over {u}"),
            None => return raw_size(ty).map(Kind::Raw),
        },
    })
}

fn raw_size(ty: &str) -> Option<usize> {
    Some(match ty {
        "UnityEngine.Vector2" | "UnityEngine.JointAngleLimits2D" | "OptionalInt" => 8,
        "UnityEngine.Vector3" => 12,
        "UnityEngine.Vector4" | "UnityEngine.Quaternion" | "UnityEngine.Color" | "UnityEngine.Rect" | "System.Guid" => 16,
        "UnityEngine.Color32" | "UnityEngine.LayerMask" => 4,
        "UnityEngine.Bounds" | "SharpAxis" => 24,
        "UnityEngine.Matrix4x4" => 64,
        "UnityEngine.ContactPoint2D" => 56,
        "UnityEngine.RaycastHit2D" => 36,
        "LiquidContainerLeakingBehaviour+Hole" => 20,
        // A blittable struct: uint, Vector3, two floats, bool (3 pad bytes), EffectSize (int), float, int, in memory order.
        "ExplosionCreator+ExplosionParameters" => 40,
        _ => return None,
    })
}

/// A boxed value's payload; the type is named beside it.
#[derive(Clone, Debug, PartialEq)]
pub enum Payload {
    Bool(bool),
    F32(u32),
    F64(u64),
    I32(i32),
    U32(u32),
    U16(u16),
    U8(u8),
    Str(Option<String>),
    /// `Kind::EnumI32`: the underlying value.
    EnumI32(i32),
    EnumU8(u8),
    Raw(Vec<u8>),
    /// `Kind::RawArray`: the item count and the memory.
    RawArray(u32, Vec<u8>),
    /// `Kind::Distributions`: (Amount bits, LiquidID).
    Distributions(Vec<(u32, Option<String>)>),
}

/// A `Dictionary<string, object>` value. The serializer's object cache compares with `object.Equals`, so a boxed value type or string that equals an earlier one (same type,
/// same value: every boxed bool, float, string, vector ... repeated) is written as a reference to it; reading resolves those back, so equal values are always
/// `Value` here and the writer decides which are new.
#[derive(Clone, Debug, PartialEq)]
pub enum Slot {
    Null,
    /// A boxed value of this type (a `System.Type` held as an object is `ty == "System.Type"` with `Payload::Str`).
    Value(String, Payload),
}

/// `MonoBehaviourPrototype.ComponentReferenceLink`: the SerialisableIdentity Guid (as stored memory) and the component type.
#[derive(Clone, Debug, PartialEq)]
pub struct CLink {
    pub Guid: [u8; 16],
    pub ComponentType: Option<String>,
}

/// `MonoBehaviourPrototype` (members in wire order: Enabled, storedReferenceFields, storedFields, storedProperties, Type).
#[derive(Clone, Debug, PartialEq)]
pub struct CPrototype {
    pub Enabled: bool,
    pub storedReferenceFields: Option<Vec<(Option<String>, CLink)>>,
    pub storedFields: Option<Vec<(Option<String>, Slot)>>,
    pub storedProperties: Option<Vec<(Option<String>, Slot)>>,
    pub Type: Option<String>,
}

/// `ObjectState` (members in wire order: RootComponentData, ChildComponentData, Children, SpawnableAssetName, Root).
#[derive(Clone, Debug, PartialEq)]
pub struct CObjectState {
    pub RootComponentData: Option<Vec<CPrototype>>,
    pub ChildComponentData: Option<Vec<(Option<String>, Option<Vec<CPrototype>>)>>,
    /// The Dictionary<string, TransformPrototype>: TransformPrototype is blittable, 24 bytes (RelativePosition, LocalScale, RelativeRotation).
    pub Children: Option<Vec<(Option<String>, [u8; 24])>>,
    pub SpawnableAssetName: Option<String>,
    pub Root: [u8; 24],
}

/// `Contraption`
#[derive(Default, Clone, Debug, PartialEq)]
pub struct CContraption {
    pub ObjectStates: Option<Vec<CObjectState>>,
}

const NULL: i32 = -1;
const NEW_VALUE: i32 = -2;
const NEW_VALUE_SAME_TYPE: i32 = -3;
const OBJECT: &str = "System.Object";

struct Reader<'a> {
    d: &'a [u8],
    o: usize,
    types: Vec<Option<String>>,
    objects: u32,
    /// The boxed values read so far by object id.
    boxed: HashMap<u32, (String, Payload)>,
}

type Res<T> = Result<T, String>;

impl<'a> Reader<'a> {
    fn byte(&mut self) -> Res<u8> {
        let b = *self.d.get(self.o).ok_or("Ceras: unexpected end of data")?;
        self.o += 1;
        Ok(b)
    }

    fn raw(&mut self, n: usize) -> Res<&'a [u8]> {
        let s = self.d.get(self.o..self.o + n).ok_or("Ceras: unexpected end of data")?;
        self.o += n;
        Ok(s)
    }

    /// SerializerBinary.ReadVarInt
    fn varint(&mut self) -> Res<u64> {
        let (mut r, mut shift) = (0u64, 0u32);
        loop {
            let b = self.byte()? as u64;
            if shift > 64 {
                return Err("Ceras: malformed VarInt".into());
            }
            r |= (b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(r);
            }
            shift += 7;
        }
    }

    /// SerializerBinary.ReadInt32 (zigzag)
    fn int32(&mut self) -> Res<i32> {
        let u = self.varint()?;
        Ok(if u & 1 != 0 { -((u >> 1) as i64 + 1) as i32 } else { (u >> 1) as i32 })
    }

    /// SerializerBinary.ReadUInt32Bias
    fn bias(&mut self, bias: i32) -> Res<i32> {
        Ok((self.varint()? as i32).wrapping_sub(bias))
    }

    /// SerializerBinary.ReadString
    fn string(&mut self) -> Res<Option<String>> {
        let n = self.bias(1)?;
        if n == -1 {
            return Ok(None);
        }
        let b = self.raw(n as usize)?;
        Ok(Some(String::from_utf8(b.to_vec()).map_err(|_| "Ceras: invalid UTF-8")?))
    }

    /// TypeFormatter.Deserialize
    fn typ(&mut self) -> Res<Option<String>> {
        let mode = self.bias(3)?;
        match mode {
            NULL => Ok(None),
            m if m >= 0 => self.types.get(m as usize).cloned().ok_or_else(|| "Ceras: bad type id".to_string()),
            // NewGeneric: the definition and the arguments first (their ids come first), then the type's proxy.
            NEW_VALUE => {
                let definition = self.typ()?.ok_or("Ceras: null generic type definition")?;
                let n = self.byte()?;
                let mut arguments = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    arguments.push(self.typ()?);
                }
                let ty = happy_type_binder_get_type_from_base_and_arguments(&definition, Some(&arguments));
                self.types.push(ty.clone());
                Ok(ty)
            }
            // NewSingle: the proxy, then the name; a type the binder gives null for stays null.
            _ => {
                let at = self.types.len();
                self.types.push(None);
                let name = self.string()?;
                let ty = happy_type_binder_get_type_from_base(name.as_deref());
                self.types[at] = ty.clone();
                Ok(ty)
            }
        }
    }

    /// The header of a ReferenceFormatter value: None for null, Some(Err(id)) for a back-reference, Some(Ok(type)) for a new object (the static type when the
    /// header says so). A new object takes the next id.
    fn head(&mut self, static_type: &str) -> Res<Option<Result<String, u32>>> {
        let k = self.bias(5)?;
        match k {
            NULL => Ok(None),
            k if k >= 0 => Ok(Some(Err(k as u32))),
            NEW_VALUE => {
                let t = self.typ()?.ok_or("Ceras: null type")?;
                self.objects += 1;
                Ok(Some(Ok(t)))
            }
            NEW_VALUE_SAME_TYPE => {
                self.objects += 1;
                Ok(Some(Ok(static_type.to_string())))
            }
            _ => Err(format!("Ceras: unsupported reference header {k}")),
        }
    }

    fn payload(&mut self, ty: &str) -> Res<Payload> {
        let k = kind(ty).ok_or_else(|| format!("Ceras: the type {ty} is not rewritten"))?;
        Ok(match k {
            Kind::Bool => Payload::Bool(self.int32()? != 0),
            Kind::F32 => Payload::F32(u32::from_le_bytes(self.raw(4)?.try_into().unwrap())),
            Kind::F64 => Payload::F64(u64::from_le_bytes(self.raw(8)?.try_into().unwrap())),
            Kind::I32 => Payload::I32(self.int32()?),
            Kind::U32 => Payload::U32(self.varint()? as u32),
            Kind::U16 => Payload::U16(u16::from_le_bytes(self.raw(2)?.try_into().unwrap())),
            Kind::U8 => Payload::U8(self.byte()?),
            Kind::Str => Payload::Str(self.string()?),
            Kind::EnumI32 => Payload::EnumI32(self.int32()?),
            Kind::EnumU8 => Payload::EnumU8(self.byte()?),
            Kind::Raw(n) => Payload::Raw(self.raw(n)?.to_vec()),
            Kind::RawArray(n) => {
                let count = self.varint()? as u32;
                Payload::RawArray(count, self.raw(n * count as usize)?.to_vec())
            }
            Kind::Distributions => {
                let n = self.bias(1)?;
                if n < 0 {
                    return Err("Ceras: null distribution array".into());
                }
                let mut v = Vec::new();
                for _ in 0..n {
                    let amount = u32::from_le_bytes(self.raw(4)?.try_into().unwrap());
                    v.push((amount, self.string()?));
                }
                Payload::Distributions(v)
            }
        })
    }

    /// ReferenceFormatter<object>
    fn slot(&mut self) -> Res<Slot> {
        // an InlineType header (a System.Type held as an object) is a new object followed by the type
        let at = self.o;
        if self.bias(5)? == -5 {
            let id = self.objects;
            self.objects += 1;
            let p = Payload::Str(self.typ()?);
            self.boxed.insert(id, ("System.Type".into(), p.clone()));
            return Ok(Slot::Value("System.Type".into(), p));
        }
        self.o = at;
        Ok(match self.head(OBJECT)? {
            None => Slot::Null,
            Some(Err(id)) => {
                let (ty, p) = self.boxed.get(&id).cloned().ok_or("Ceras: a reference to an object that is not a boxed value")?;
                if matches!(p, Payload::RawArray(..) | Payload::Distributions(_)) {
                    return Err("Ceras: a shared array is not rewritten".into());
                }
                Slot::Value(ty, p)
            }
            Some(Ok(ty)) => {
                let id = self.objects - 1;
                let p = self.payload(&ty)?;
                self.boxed.insert(id, (ty.clone(), p.clone()));
                Slot::Value(ty, p)
            }
        })
    }

    fn dict<V>(&mut self, mut value: impl FnMut(&mut Self) -> Res<V>) -> Res<Option<Vec<(Option<String>, V)>>> {
        let Some(h) = self.head("Dictionary")? else { return Ok(None) };
        if h.is_err() {
            return Err("Ceras: shared dictionaries are not rewritten".into());
        }
        let n = self.varint()?;
        let mut out = Vec::new();
        for _ in 0..n {
            let key = self.string()?;
            out.push((key, value(self)?));
        }
        Ok(Some(out))
    }

    fn link(&mut self) -> Res<CLink> {
        let Guid: [u8; 16] = self.raw(16)?.try_into().unwrap();
        Ok(CLink { Guid, ComponentType: self.typ()? })
    }

    fn prototype(&mut self) -> Res<Option<CPrototype>> {
        let Some(h) = self.head("MonoBehaviourPrototype")? else { return Ok(None) };
        if h.is_err() {
            return Err("Ceras: shared prototypes are not rewritten".into());
        }
        let Enabled = self.int32()? != 0;
        let storedReferenceFields = self.dict(|r| r.link())?;
        let storedFields = self.dict(|r| r.slot())?;
        let storedProperties = self.dict(|r| r.slot())?;
        let Type = self.typ()?;
        Ok(Some(CPrototype { Enabled, storedReferenceFields, storedFields, storedProperties, Type }))
    }

    fn prototypes(&mut self) -> Res<Option<Vec<CPrototype>>> {
        let Some(h) = self.head("MonoBehaviourPrototype[]")? else { return Ok(None) };
        if h.is_err() {
            return Err("Ceras: shared arrays are not rewritten".into());
        }
        let n = self.bias(1)?;
        let mut out = Vec::new();
        for _ in 0..n {
            out.push(self.prototype()?.ok_or("Ceras: null prototype")?);
        }
        Ok(Some(out))
    }

    fn object_state(&mut self) -> Res<Option<CObjectState>> {
        let Some(h) = self.head("ObjectState")? else { return Ok(None) };
        if h.is_err() {
            return Err("Ceras: shared object states are not rewritten".into());
        }
        let RootComponentData = self.prototypes()?;
        let ChildComponentData = self.dict(|r| r.prototypes())?;
        let Children = self.dict(|r| Ok::<[u8; 24], String>(r.raw(24)?.try_into().unwrap()))?;
        let SpawnableAssetName = self.string()?;
        let Root = self.raw(24)?.try_into().unwrap();
        Ok(Some(CObjectState { RootComponentData, ChildComponentData, Children, SpawnableAssetName, Root }))
    }

    fn contraption(&mut self) -> Res<Option<CContraption>> {
        let Some(h) = self.head("Contraption")? else { return Ok(None) };
        if h.is_err() {
            return Err("Ceras: shared contraptions are not rewritten".into());
        }
        let Some(h) = self.head("ObjectState[]")? else { return Ok(Some(CContraption { ObjectStates: None })) };
        if h.is_err() {
            return Err("Ceras: shared arrays are not rewritten".into());
        }
        let n = self.bias(1)?;
        let mut v = Vec::new();
        for _ in 0..n {
            v.push(self.object_state()?.ok_or("Ceras: null object state")?);
        }
        Ok(Some(CContraption { ObjectStates: Some(v) }))
    }
}

/// CerasSerializer.Deserialize<Contraption>: None for a null root. The whole buffer must be consumed.
pub fn decode_contraption(bytes: &[u8]) -> Res<Option<CContraption>> {
    let mut r = Reader { d: bytes, o: 0, types: Vec::new(), objects: 0, boxed: HashMap::new() };
    let c = r.contraption()?;
    if r.o != bytes.len() {
        return Err(format!("Ceras: {} bytes left over", bytes.len() - r.o));
    }
    Ok(c)
}

struct Writer {
    b: Vec<u8>,
    types: HashMap<String, u32>,
    ntypes: u32,
    objects: u32,
    /// The object cache's boxed values (by type and value) and their ids.
    cache: HashMap<String, u32>,
}

impl Writer {
    /// SerializerBinary.WriteByte
    fn byte(&mut self, v: u8) {
        self.b.push(v);
    }

    fn varint(&mut self, mut v: u64) {
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v != 0 {
                self.b.push(byte | 0x80);
            } else {
                self.b.push(byte);
                return;
            }
        }
    }

    /// SerializerBinary.WriteInt32 (zigzag)
    fn int32(&mut self, v: i32) {
        self.varint((((v as i64) << 1) ^ ((v as i64) >> 31)) as u64);
    }

    /// SerializerBinary.WriteUInt32Bias
    fn bias(&mut self, v: i32, bias: i32) {
        self.varint((v + bias) as u32 as u64);
    }

    /// SerializerBinary.WriteString
    fn string(&mut self, s: &Option<String>) {
        match s {
            None => self.bias(-1, 1),
            Some(s) => {
                self.bias(s.len() as i32, 1);
                self.b.extend_from_slice(s.as_bytes());
            }
        }
    }

    /// TypeFormatter.Serialize, with HappyTypeBinder.GetBaseName: the type's FullName (the names here are FullNames; no
    /// generic type is written).
    fn typ(&mut self, t: &Option<String>) {
        let Some(t) = t else { return self.bias(NULL, 3) };
        if let Some(id) = self.types.get(t) {
            let id = *id as i32;
            return self.bias(id, 3);
        }
        if let Some((definition, arguments)) = generic_parts(t) {
            self.bias(NEW_VALUE, 3);
            self.typ(&Some(definition.to_string()));
            self.byte(arguments.len() as u8);
            for a in arguments {
                self.typ(&Some(a.to_string()));
            }
        } else {
            self.bias(-3, 3);
            self.string(&Some(t.clone()));
        }
        self.types.insert(t.clone(), self.ntypes);
        self.ntypes += 1;
    }

    /// A new object of the static type: the next id.
    fn new_same(&mut self) {
        self.bias(NEW_VALUE_SAME_TYPE, 5);
        self.objects += 1;
    }

    fn payload(&mut self, ty: &str, p: &Payload) -> Res<()> {
        match (kind(ty), p) {
            (Some(Kind::Bool), Payload::Bool(v)) => self.int32(*v as i32),
            (Some(Kind::F32), Payload::F32(v)) => self.b.extend_from_slice(&v.to_le_bytes()),
            (Some(Kind::F64), Payload::F64(v)) => self.b.extend_from_slice(&v.to_le_bytes()),
            (Some(Kind::I32), Payload::I32(v)) => self.int32(*v),
            (Some(Kind::U32), Payload::U32(v)) => self.varint(*v as u64),
            (Some(Kind::U16), Payload::U16(v)) => self.b.extend_from_slice(&v.to_le_bytes()),
            (Some(Kind::U8), Payload::U8(v)) => self.b.push(*v),
            (Some(Kind::Str), Payload::Str(s)) => self.string(s),
            (Some(Kind::EnumI32), Payload::EnumI32(v)) => self.int32(*v),
            (Some(Kind::EnumU8), Payload::EnumU8(v)) => self.b.push(*v),
            (Some(Kind::Raw(n)), Payload::Raw(v)) if v.len() == n => self.b.extend_from_slice(v),
            (Some(Kind::RawArray(n)), Payload::RawArray(count, v)) if v.len() == n * *count as usize => {
                self.varint(*count as u64);
                self.b.extend_from_slice(v);
            }
            (Some(Kind::Distributions), Payload::Distributions(v)) => {
                self.bias(v.len() as i32, 1);
                for (amount, id) in v {
                    self.b.extend_from_slice(&amount.to_le_bytes());
                    self.string(id);
                }
            }
            _ => return Err(format!("Ceras: the value of type {ty} does not match its payload")),
        }
        Ok(())
    }

    /// ReferenceFormatter<object>.Serialize
    fn slot(&mut self, s: &Slot) -> Res<()> {
        let Slot::Value(ty, p) = s else { return Ok(self.bias(NULL, 5)) };
        // arrays are references (compared by identity, never equal); strings and boxed value types are compared by value
        if !matches!(p, Payload::RawArray(..) | Payload::Distributions(_)) {
            // Single.Equals / Double.Equals (and corefx's GetHashCode, which masks NaN and +-0 alike): every NaN is equal to every
            // other NaN and -0 to +0, so a boxed ffc00000 after a 7fc00000 is a reference to the first.
            let key = match p {
                Payload::F32(x) if f32::from_bits(*x).is_nan() => format!("{ty}|F32(nan)"),
                Payload::F32(x) if f32::from_bits(*x) == 0.0 => format!("{ty}|F32(0)"),
                Payload::F64(x) if f64::from_bits(*x).is_nan() => format!("{ty}|F64(nan)"),
                Payload::F64(x) if f64::from_bits(*x) == 0.0 => format!("{ty}|F64(0)"),
                _ => format!("{ty}|{p:?}"),
            };
            if let Some(id) = self.cache.get(&key) {
                let id = *id as i32;
                self.bias(id, 5);
                return Ok(());
            }
            self.cache.insert(key, self.objects);
        }
        self.objects += 1;
        if ty == "System.Type" {
            // an InlineType value
            self.bias(-5, 5);
            let Payload::Str(name) = p else { return Err("Ceras: a Type value needs a name".into()) };
            self.typ(name);
        } else {
            self.bias(NEW_VALUE, 5);
            self.typ(&Some(ty.clone()));
            self.payload(ty, p)?;
        }
        Ok(())
    }

    fn dict<V>(&mut self, d: &Option<Vec<(Option<String>, V)>>, mut value: impl FnMut(&mut Self, &V) -> Res<()>) -> Res<()> {
        let Some(d) = d else { return Ok(self.bias(NULL, 5)) };
        self.new_same();
        self.varint(d.len() as u64);
        for (k, v) in d {
            self.string(k);
            value(self, v)?;
        }
        Ok(())
    }

    fn prototype(&mut self, p: &CPrototype) -> Res<()> {
        self.new_same();
        self.int32(p.Enabled as i32);
        self.dict(&p.storedReferenceFields, |w, l| {
            w.b.extend_from_slice(&l.Guid);
            w.typ(&l.ComponentType);
            Ok(())
        })?;
        self.dict(&p.storedFields, |w, s| w.slot(s))?;
        self.dict(&p.storedProperties, |w, s| w.slot(s))?;
        self.typ(&p.Type);
        Ok(())
    }

    fn prototypes(&mut self, a: &Option<Vec<CPrototype>>) -> Res<()> {
        let Some(a) = a else { return Ok(self.bias(NULL, 5)) };
        self.new_same();
        self.bias(a.len() as i32, 1);
        for p in a {
            self.prototype(p)?;
        }
        Ok(())
    }

    fn object_state(&mut self, o: &CObjectState) -> Res<()> {
        self.new_same();
        self.prototypes(&o.RootComponentData)?;
        self.dict(&o.ChildComponentData, |w, a| w.prototypes(a))?;
        self.dict(&o.Children, |w, t| Ok(w.b.extend_from_slice(t)))?;
        self.string(&o.SpawnableAssetName);
        self.b.extend_from_slice(&o.Root);
        Ok(())
    }
}

/// CerasSerializer.Serialize<Contraption>
pub fn encode_contraption(c: &Option<CContraption>) -> Res<Vec<u8>> {
    let mut w = Writer { b: Vec::new(), types: HashMap::new(), ntypes: 0, objects: 0, cache: HashMap::new() };
    match c {
        None => w.bias(NULL, 5),
        Some(c) => {
            w.new_same();
            match &c.ObjectStates {
                None => w.bias(NULL, 5),
                Some(states) => {
                    w.new_same();
                    w.bias(states.len() as i32, 1);
                    for s in states {
                        w.object_state(s)?;
                    }
                }
            }
        }
    }
    Ok(w.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_type_binder() {
        assert_eq!(happy_type_binder_get_type_from_base(Some("System.Single, mscorlib, Version=4.0.0.0")).as_deref(), Some("System.Single"));
        assert_eq!(happy_type_binder_get_type_from_base(Some("System.IO.FileInfo")), None);
        assert_eq!(happy_type_binder_get_type_from_base(Some(" ")), None);
        assert_eq!(strip_assembly_info("A`1[[B, x]], y"), "A`1[[B, x]]");
        let args = [Some("System.Single".to_string()), Some("System.Collections.Generic.List`1[[System.Int32]]".to_string())];
        let d = happy_type_binder_get_type_from_base_and_arguments("System.Collections.Generic.Dictionary`2", Some(&args)).unwrap();
        assert_eq!(d, "System.Collections.Generic.Dictionary`2[[System.Single],[System.Collections.Generic.List`1[[System.Int32]]]]");
        assert_eq!(generic_parts(&d), Some(("System.Collections.Generic.Dictionary`2", vec!["System.Single", "System.Collections.Generic.List`1[[System.Int32]]"])));
        assert_eq!(happy_type_binder_get_type_from_base_and_arguments("System.Collections.Generic.List`1", Some(&args)), None);
        assert_eq!(happy_type_binder_get_type_from_base_and_arguments("System.Collections.Generic.List`1", Some(&[Some("System.IO.File".into())])), None);
        // TypeFormatter: written and read back, with the ids the parts take first.
        let mut w = Writer { b: Vec::new(), types: HashMap::new(), ntypes: 0, objects: 0, cache: HashMap::new() };
        w.typ(&Some(d.clone()));
        w.typ(&Some(d.clone()));
        w.typ(&Some("System.Single".into()));
        let mut r = Reader { d: &w.b, o: 0, types: Vec::new(), objects: 0, boxed: HashMap::new() };
        assert_eq!((r.typ().unwrap(), r.typ().unwrap(), r.typ().unwrap()), (Some(d.clone()), Some(d), Some("System.Single".into())));
        assert_eq!((r.o, r.types.len()), (w.b.len(), 6));
    }

    /// Every real contraption file (the installed game's Contraptions folder) decodes and encodes back to the same bytes; a file with a type not
    /// rewritten is skipped.
    #[test]
    fn real_files_round_trip() {
        let files = super::super::contraption::tests::fixtures();
        for f in files {
            let bytes = std::fs::read(&f).unwrap();
            // The game's own write-back differs from the file when a type the binder can't resolve (UnityEngine.AudioSource) is
            // kept as "" (66 of the developer's 73 saves; SaveContraption's rows check it against the game): what it writes reads back the same.
            let c = decode_contraption(&bytes).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
            let once = encode_contraption(&c).unwrap();
            assert_eq!(encode_contraption(&decode_contraption(&once).unwrap()).unwrap(), once, "{} does not write back to the same bytes", f.display());
        }
    }
}
