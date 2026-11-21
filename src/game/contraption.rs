//! Contraption save and load: ContraptionSerialiser (the `.jaap` Ceras bytes, the `.json` ContraptionMetaData, the `.png` thumbnail),
//! ContraptionFolderSource, PollingDirectoryWatcher, ContraptionSpriteStorage, ContraptionSavePoint, the delete button, the context
//! menu's SaveAction checks, ContraptionOutlineSerialiser's text files and MigrationEvent. The bytes come from `ceras.rs` (the
//! Ceras 4.0.40 wire format, checked against real `.jaap` files); this file maps them to and from `save::ObjectState`.
//!
//! Not rewritten: the dialogs and notifications' look (UI), the thumbnail picture (ThumbnailCreator renders it; the PNG bytes
//! come in from the caller), Steam (unsubscribe, stats) and the mod list a contraption requires (mods are out of scope).
use super::ceras::{self, CContraption, CLink, CObjectState, CPrototype, Payload, Slot};
use super::save::{ComponentReferenceLink, MonoBehaviourPrototype, ObjectState, SavedValue, TransformPrototype};
use super::save_members::{ENUM_TYPES, MEMBER_TYPES};
use super::*;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------------------------------------------------------------
// ObjectState <-> the Ceras graph
// ---------------------------------------------------------------------------------------------------------------------------------

/// The .NET type of a public member (`PUBLIC_MEMBERS`), if the class has it.
fn member_type(class: &str, name: &str) -> Option<&'static str> {
    // A namespaced component (Linefy.Primitives.LinefyCircularPolyline) is listed by its class name.
    let short = class.rsplit('.').next().unwrap_or(class);
    let v45 = if crate::assets::il::is_1_28_45() { super::save_members_45::MEMBER_TYPES } else { &[] };
    MEMBER_TYPES.iter().chain(v45).find(|(c, n, _)| (*c == class || *c == short) && *n == name).map(|(_, _, t)| *t)
}

fn f32s_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_bits().to_le_bytes()).collect()
}

fn bytes_to_f32s(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_bits(u32::from_le_bytes([c[0], c[1], c[2], c[3]]))).collect()
}

/// A SharpAxis in managed memory (20 bytes): Axis, LowerLimit, UpperLimit, LooseUpperLimit and LooseLowerLimit (1 byte each), 2 padding bytes.
/// Ceras blits Marshal.SizeOf (24, a bool marshals as 4 bytes) per item from the array, so an n-item blob is the n items at a
/// stride of 20, then 4n bytes of the memory past the array (not modelled: written as 0, ignored on read).
fn axis_bytes(a: &([f32; 2], f32, f32, bool, bool)) -> [u8; 20] {
    let mut b = [0u8; 20];
    b[..16].copy_from_slice(&f32s_to_bytes(&[a.0[0], a.0[1], a.1, a.2]));
    b[16] = a.3 as u8;
    b[17] = a.4 as u8;
    b
}

fn axis_from_bytes(b: &[u8]) -> ([f32; 2], f32, f32, bool, bool) {
    let f = bytes_to_f32s(&b[..16]);
    ([f[0], f[1]], f[2], f[3], b[16] != 0, b[17] != 0)
}

fn field_to_payload(class: &str, name: &str, text: &str) -> Result<(String, Payload), String> {
    let ty = member_type(class, name).ok_or(format!("{class}.{name} has no known .NET type"))?;
    let bad = || format!("{class}.{name}: bad value {text}");
    let hex = |t: &str| u32::from_str_radix(t, 16).map_err(|_| bad());
    let p = match ty {
        "System.Boolean" => Payload::Bool(text == "1"),
        "System.Single" => Payload::F32(hex(text)?),
        "System.Int32" => Payload::I32(text.parse().map_err(|_| bad())?),
        "System.UInt32" => Payload::U32(text.parse().map_err(|_| bad())?),
        "System.UInt16" => Payload::U16(text.parse().map_err(|_| bad())?),
        "System.Byte" => Payload::U8(text.parse().map_err(|_| bad())?),
        "System.Double" => Payload::F64(u64::from_str_radix(text, 16).map_err(|_| bad())?),
        "System.String" => Payload::Str(Some(text.to_string())),
        // LayerMask is its int bits; Vector3Int three ints.
        "UnityEngine.LayerMask" => Payload::Raw(text.parse::<i32>().map_err(|_| bad())?.to_le_bytes().to_vec()),
        "UnityEngine.Vector3Int" => {
            let mut bytes = Vec::new();
            for t in text.split(',') {
                bytes.extend(t.parse::<i32>().map_err(|_| bad())?.to_le_bytes());
            }
            Payload::Raw(bytes)
        }
        "UnityEngine.Vector2" | "UnityEngine.Vector3" | "UnityEngine.Color" | "UnityEngine.Bounds" | "UnityEngine.Rect" => {
            let mut bytes = Vec::new();
            for t in text.split(',') {
                bytes.extend(hex(t)?.to_le_bytes());
            }
            Payload::Raw(bytes)
        }
        "UnityEngine.Vector3[]" | "UnityEngine.Vector4[]" => {
            let n = if ty == "UnityEngine.Vector3[]" { 3 } else { 4 };
            let mut bytes = Vec::new();
            for t in text.split(',').filter(|t| !t.is_empty()) {
                bytes.extend(hex(t)?.to_le_bytes());
            }
            Payload::RawArray((bytes.len() / 4 / n) as u32, bytes)
        }
        // Enums of the engine itself (int-backed).
        "UnityEngine.KeyCode" | "UnityEngine.ForceMode2D" | "UnityEngine.RectTransform+Axis" => Payload::EnumI32(text.parse().map_err(|_| bad())?),
        _ => match ENUM_TYPES.iter().find(|(e, _)| *e == ty) {
            Some((_, "System.Byte")) => Payload::EnumU8(text.parse().map_err(|_| bad())?),
            Some(_) => Payload::EnumI32(text.parse().map_err(|_| bad())?),
            None => return Err(format!("{class}.{name}: {ty} is not a text value")),
        },
    };
    Ok((ty.to_string(), p))
}

fn saved_to_slot(class: &str, name: &str, v: &SavedValue) -> Result<Slot, String> {
    let guid = |g: &u128| g.to_le_bytes().to_vec();
    Ok(match v {
        SavedValue::Null => Slot::Null,
        SavedValue::Field(t) => {
            let (ty, p) = field_to_payload(class, name, t)?;
            Slot::Value(ty, p)
        }
        SavedValue::Guid(g) => Slot::Value("System.Guid".into(), Payload::Raw(guid(g))),
        SavedValue::Guids(g) => Slot::Value("System.Guid[]".into(), Payload::RawArray(g.len() as u32, g.iter().flat_map(guid).collect())),
        SavedValue::Floats(f) => Slot::Value("System.Single[]".into(), Payload::RawArray(f.len() as u32, f32s_to_bytes(f))),
        SavedValue::Vec2s(v) => Slot::Value("UnityEngine.Vector2[]".into(), Payload::RawArray(v.len() as u32, v.iter().flat_map(|p| f32s_to_bytes(p)).collect())),
        SavedValue::Bools(b) => Slot::Value("System.Boolean[]".into(), Payload::RawArray(b.len() as u32, b.iter().map(|&x| x as u8).chain(std::iter::repeat_n(0, 3 * b.len())).collect())),
        SavedValue::Axes(a, tail) => Slot::Value("SharpAxis[]".into(), Payload::RawArray(a.len() as u32, a.iter().flat_map(|x| axis_bytes(x)).chain(tail.iter().copied()).collect())),
        SavedValue::Distributions(d) => {
            Slot::Value("BloodContainer+SerialisableDistribution[]".into(), Payload::Distributions(d.iter().map(|(id, amount)| (amount.to_bits(), Some(id.clone()))).collect()))
        }
        SavedValue::Holes(h) => Slot::Value("LiquidContainerLeakingBehaviour+Hole[]".into(), Payload::RawArray(h.len() as u32, h.iter().flat_map(|x| f32s_to_bytes(x)).collect())),
        SavedValue::Net(ty, p) => Slot::Value(ty.clone(), p.clone()),
    })
}

fn slot_to_saved(class: &str, name: &str, slot: &Slot) -> SavedValue {
    let Slot::Value(ty, p) = slot else { return SavedValue::Null };
    let net = || SavedValue::Net(ty.clone(), p.clone());
    let item = |bytes: &[u8], n: usize| bytes.chunks_exact(n).map(|c| c.to_vec()).collect::<Vec<_>>();
    let guid = |c: &[u8]| u128::from_le_bytes(c.try_into().unwrap());
    let saved = match (ty.as_str(), p) {
        ("System.Guid", Payload::Raw(b)) => SavedValue::Guid(guid(b)),
        ("System.Guid[]", Payload::RawArray(_, b)) => SavedValue::Guids(item(b, 16).iter().map(|c| guid(c)).collect()),
        ("System.Single[]", Payload::RawArray(_, b)) => SavedValue::Floats(bytes_to_f32s(b)),
        ("UnityEngine.Vector2[]", Payload::RawArray(_, b)) => SavedValue::Vec2s(bytes_to_f32s(b).chunks_exact(2).map(|c| [c[0], c[1]]).collect()),
        ("LiquidContainerLeakingBehaviour+Hole[]", Payload::RawArray(_, b)) => SavedValue::Holes(bytes_to_f32s(b).chunks_exact(5).map(|c| [c[0], c[1], c[2], c[3], c[4]]).collect()),
        // Ceras blits 4 bytes per bool from the 1-byte items: the first `count` bytes are the items, the rest is memory past the array.
        // Keep the typed form only when that tail is zero (SavedValue::bools reads the items of a kept one).
        ("System.Boolean[]", Payload::RawArray(n, b)) if b.len() == 4 * *n as usize && b[..*n as usize].iter().all(|&x| x < 2) && b[*n as usize..].iter().all(|&x| x == 0) => {
            SavedValue::Bools(b[..*n as usize].iter().map(|&x| x == 1).collect())
        }
        ("SharpAxis[]", Payload::RawArray(n, b)) if b.len() == 24 * *n as usize => {
            // The items at a stride of 20, then the bytes from past the array (kept: re-serialising the loaded state writes them back).
            let k = 20 * *n as usize;
            SavedValue::Axes(b[..k].chunks_exact(20).map(axis_from_bytes).collect(), b[k..].to_vec())
        }
        // Hole is unmanaged (LocalPos, LocalDir, Intensity): its memory, 20 bytes each.
        ("LiquidContainerLeakingBehaviour+Hole[]", Payload::RawArray(_, b)) if b.len() % 20 == 0 => {
            SavedValue::Holes(bytes_to_f32s(b).chunks_exact(5).map(|c| [c[0], c[1], c[2], c[3], c[4]]).collect())
        }
        ("BloodContainer+SerialisableDistribution[]", Payload::Distributions(d)) if d.iter().all(|(_, id)| id.is_some()) => {
            SavedValue::Distributions(d.iter().map(|(a, id)| (id.clone().unwrap(), f32::from_bits(*a))).collect())
        }
        _ if member_type(class, name) == Some(ty.as_str()) => match p {
            Payload::Bool(b) => SavedValue::Field(if *b { "1" } else { "0" }.into()),
            Payload::F32(b) => SavedValue::Field(format!("{b:08x}")),
            Payload::I32(v) | Payload::EnumI32(v) => SavedValue::Field(v.to_string()),
            Payload::U32(v) => SavedValue::Field(v.to_string()),
            Payload::U16(v) => SavedValue::Field(v.to_string()),
            Payload::U8(v) => SavedValue::Field(v.to_string()),
            Payload::EnumU8(v) => SavedValue::Field(v.to_string()),
            Payload::F64(v) => SavedValue::Field(format!("{v:016x}")),
            Payload::Str(Some(s)) => SavedValue::Field(s.clone()),
            Payload::Raw(b) if ty == "UnityEngine.LayerMask" && b.len() == 4 => SavedValue::Field(i32::from_le_bytes([b[0], b[1], b[2], b[3]]).to_string()),
            Payload::Raw(b) if ty == "UnityEngine.Vector3Int" && b.len() == 12 => {
                SavedValue::Field(b.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]).to_string()).collect::<Vec<_>>().join(","))
            }
            Payload::Raw(b) if matches!(ty.as_str(), "UnityEngine.Vector2" | "UnityEngine.Vector3" | "UnityEngine.Color" | "UnityEngine.Bounds" | "UnityEngine.Rect") => {
                SavedValue::Field(bytes_to_f32s(b).iter().map(|f| format!("{:08x}", f.to_bits())).collect::<Vec<_>>().join(","))
            }
            Payload::RawArray(_, b) if matches!(ty.as_str(), "UnityEngine.Vector3[]" | "UnityEngine.Vector4[]") => {
                SavedValue::Field(bytes_to_f32s(b).iter().map(|f| format!("{:08x}", f.to_bits())).collect::<Vec<_>>().join(","))
            }
            _ => return net(),
        },
        _ => return net(),
    };
    saved
}

fn transform_to_bytes(t: &TransformPrototype) -> [u8; 24] {
    let f = [t.RelativePosition[0], t.RelativePosition[1], t.LocalScale[0], t.LocalScale[1], t.LocalScale[2], t.RelativeRotation];
    f32s_to_bytes(&f).try_into().unwrap()
}

fn transform_from_bytes(b: &[u8; 24]) -> TransformPrototype {
    let f = bytes_to_f32s(b);
    TransformPrototype { RelativePosition: [f[0], f[1]], LocalScale: [f[2], f[3], f[4]], RelativeRotation: f[5] }
}

fn prototype_to_ceras(p: &MonoBehaviourPrototype) -> Result<CPrototype, String> {
    let members = |m: &[(String, SavedValue)]| -> Result<Vec<(Option<String>, Slot)>, String> {
        m.iter().map(|(n, v)| Ok((Some(n.clone()), saved_to_slot(&p.Type, n, v)?))).collect()
    };
    Ok(CPrototype {
        Enabled: p.Enabled,
        storedReferenceFields: Some(p.storedReferenceFields.iter().map(|(n, l)| (Some(n.clone()), CLink { Guid: l.UniqueIdentity.to_le_bytes(), ComponentType: l.ComponentType.clone() })).collect()),
        storedFields: Some(members(&p.storedFields)?),
        storedProperties: Some(members(&p.storedProperties)?),
        Type: (!p.Type.is_empty()).then(|| p.Type.clone()),
    })
}

fn prototype_from_ceras(p: &CPrototype) -> Result<MonoBehaviourPrototype, String> {
    // A Type the binder gave null for (no blank name binds) is kept as "", and written back as null, as the game keeps it.
    let class = p.Type.clone().unwrap_or_default();
    let members = |m: &Option<Vec<(Option<String>, Slot)>>| -> Result<Vec<(String, SavedValue)>, String> {
        m.as_ref().ok_or("Ceras: a stored member dictionary is null")?.iter().map(|(n, s)| {
            let n = n.clone().ok_or("Ceras: a null member name")?;
            let v = slot_to_saved(&class, &n, s);
            Ok((n, v))
        }).collect()
    };
    let refs = p.storedReferenceFields.as_ref().ok_or("Ceras: storedReferenceFields is null")?;
    Ok(MonoBehaviourPrototype {
        Enabled: p.Enabled,
        storedFields: members(&p.storedFields)?,
        storedProperties: members(&p.storedProperties)?,
        storedReferenceFields: refs.iter().map(|(n, l)| Ok((n.clone().ok_or("Ceras: a null member name")?, ComponentReferenceLink { UniqueIdentity: u128::from_le_bytes(l.Guid), ComponentType: l.ComponentType.clone() }))).collect::<Result<_, String>>()?,
        Type: class,
    })
}

/// `Contraption(ObjectState[])` as Ceras sees it.
pub fn contraption_to_ceras(states: &[ObjectState]) -> Result<CContraption, String> {
    let protos = |l: &[MonoBehaviourPrototype]| l.iter().map(prototype_to_ceras).collect::<Result<Vec<_>, _>>();
    let mut out = Vec::new();
    for s in states {
        out.push(CObjectState {
            RootComponentData: Some(protos(&s.RootComponentData)?),
            ChildComponentData: Some(s.ChildComponentData.iter().map(|(k, v)| Ok((Some(k.clone()), Some(protos(v)?)))).collect::<Result<Vec<_>, String>>()?),
            Children: Some(s.Children.iter().map(|(k, t)| (Some(k.clone()), transform_to_bytes(t))).collect()),
            SpawnableAssetName: Some(s.SpawnableAssetName.clone()),
            Root: transform_to_bytes(&s.Root),
        });
    }
    Ok(CContraption { ObjectStates: Some(out) })
}

/// The ObjectStates of a Ceras `Contraption`. A null list or name cannot be held by the structs (the game itself fails on one later, in Reconstruct), so it is an error.
pub fn contraption_from_ceras(c: &CContraption) -> Result<Vec<ObjectState>, String> {
    let protos = |l: &Option<Vec<CPrototype>>| -> Result<Vec<MonoBehaviourPrototype>, String> {
        l.as_ref().ok_or("Ceras: a component list is null")?.iter().map(prototype_from_ceras).collect()
    };
    let Some(states) = &c.ObjectStates else { return Err("Ceras: ObjectStates is null".into()) };
    states.iter().map(|s| {
        let (Some(name), Some(children), Some(child_data)) = (&s.SpawnableAssetName, &s.Children, &s.ChildComponentData) else { return Err("Ceras: an ObjectState member is null".into()) };
        Ok(ObjectState {
            SpawnableAssetName: name.clone(),
            Root: transform_from_bytes(&s.Root),
            Children: children.iter().map(|(k, t)| Ok((k.clone().ok_or("Ceras: a null key")?, transform_from_bytes(t)))).collect::<Result<_, String>>()?,
            RootComponentData: protos(&s.RootComponentData)?,
            ChildComponentData: child_data.iter().map(|(k, v)| Ok((k.clone().ok_or("Ceras: a null key")?, protos(v)?))).collect::<Result<_, String>>()?,
        })
    }).collect()
}

/// `CerasSerializer.Serialize(new Contraption(states))`
pub fn serialise_states(states: &[ObjectState]) -> Result<Vec<u8>, String> {
    ceras::encode_contraption(&Some(contraption_to_ceras(states)?))
}

/// `CerasSerializer.Deserialize<Contraption>(bytes)`
pub fn deserialise_states(bytes: &[u8]) -> Result<Vec<ObjectState>, String> {
    match ceras::decode_contraption(bytes)? {
        Some(c) => contraption_from_ceras(&c),
        None => Err("Ceras: the contraption is null".into()),
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// ContraptionMetaData
// ---------------------------------------------------------------------------------------------------------------------------------

/// RequiredMod
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RequiredMod {
    #[serde(default)]
    pub Name: Option<String>,
    #[serde(default)]
    pub Identifier: Option<String>,
    #[serde(default)]
    pub WorkshopId: Option<String>,
}

impl RequiredMod {
    /// RequiredMod.get_IsFromWorkshop
    pub fn IsFromWorkshop(&self) -> bool {
        self.WorkshopId.as_deref().map_or(false, |s| !s.is_empty())
    }
}

/// ContraptionMetaData: the serialised members (Json.NET order: fields, then the get-only properties); the [NonSerialized] paths are kept beside them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContraptionMetaData {
    pub Name: Option<String>,
    pub DisplayName: Option<String>,
    pub Version: Option<String>,
    pub PublishedFileID: u64,
    pub CreatorUGCIdentity: Option<String>,
    pub RequiredMods: Option<Vec<RequiredMod>>,
    pub Tags: Vec<String>,
    pub PathToMetadata: Option<PathBuf>,
    pub PathToDataFile: Option<PathBuf>,
    pub PathToThumbnail: Option<PathBuf>,
    pub PathToOutlineFile: Option<PathBuf>,
    pub Directory: Option<PathBuf>,
}

/// The JSON file: a key the file lacks keeps the constructor's value (Json.NET builds the object with the parameterless constructor).
#[derive(serde::Serialize, serde::Deserialize)]
struct MetaJson {
    #[serde(default)]
    Name: Option<String>,
    #[serde(default)]
    DisplayName: Option<String>,
    #[serde(default)]
    Version: Option<String>,
    #[serde(default)]
    PublishedFileID: u64,
    #[serde(default)]
    CreatorUGCIdentity: Option<String>,
    #[serde(default)]
    RequiredMods: Option<Vec<RequiredMod>>,
    #[serde(skip_deserializing, default)]
    IsCurrentVersion: bool,
    #[serde(skip_deserializing, default)]
    IsWorkshopItem: bool,
    #[serde(skip_deserializing, default)]
    RequiresMods: bool,
}

impl ContraptionMetaData {
    pub const FileExtension: &'static str = ".json";

    /// Name as C# string concatenation reads it: "" for null.
    pub fn name(&self) -> &str {
        self.Name.as_deref().unwrap_or("")
    }

    /// Version as C# string concatenation reads it: "" for null.
    pub fn version(&self) -> &str {
        self.Version.as_deref().unwrap_or("")
    }

    /// ContraptionMetaData.constructor (the parameterless one Json.NET uses): RequiredMods is an empty list.
    pub fn empty() -> Self {
        ContraptionMetaData { RequiredMods: Some(Vec::new()), ..Default::default() }
    }

    /// ContraptionMetaData.constructor(string name)
    pub fn new(name: &str) -> Self {
        ContraptionMetaData { Name: Some(name.to_string()), DisplayName: Some(name.to_string()), Version: Some(current_version().into()), RequiredMods: Some(Vec::new()), ..Default::default() }
    }

    /// ContraptionMetaData.get_IsCurrentVersion
    pub fn IsCurrentVersion(&self) -> bool {
        self.Version.as_deref() == Some(current_version())
    }

    /// ContraptionMetaData.get_IsWorkshopItem
    pub fn IsWorkshopItem(&self) -> bool {
        self.PublishedFileID != 0
    }

    /// ContraptionMetaData.get_RequiresMods
    pub fn RequiresMods(&self) -> bool {
        self.RequiredMods.as_ref().map_or(false, |m| !m.is_empty())
    }

    /// `JsonConvert.SerializeObject(metaData)`
    pub fn to_json(&self) -> String {
        serde_json::to_string(&MetaJson {
            Name: self.Name.clone(),
            DisplayName: self.DisplayName.clone(),
            Version: self.Version.clone(),
            PublishedFileID: self.PublishedFileID,
            CreatorUGCIdentity: self.CreatorUGCIdentity.clone(),
            RequiredMods: self.RequiredMods.clone(),
            IsCurrentVersion: self.IsCurrentVersion(),
            IsWorkshopItem: self.IsWorkshopItem(),
            RequiresMods: self.RequiresMods(),
        })
        .unwrap()
    }

    /// `JsonConvert.DeserializeObject<ContraptionMetaData>(text)` (jsonnet): None for JSON `null` or no value. The parameterless
    /// constructor's object, then each serialised member a property names (any case, the last winning); the get-only
    /// properties and unknown names are skipped. A string member reads as ReadAsString, PublishedFileID as a ulong, RequiredMods
    /// as a list (null, or objects: a null item is an error, RequiredMod being a struct).
    pub fn from_json(text: &str) -> Result<Option<Self>, String> {
        use super::jsonnet::{self, JTok};
        let root = match jsonnet::parse(text.trim_start_matches('\u{feff}'))? {
            None | Some(JTok::Null) | Some(JTok::Undefined) => return Ok(None),
            Some(r @ JTok::Object(_)) => r,
            Some(_) => return Err("Cannot deserialize the JSON value into type 'ContraptionMetaData'".into()),
        };
        let mut m = Self::empty();
        if let Some(v) = root.read("Name", JTok::as_string)? { m.Name = v }
        if let Some(v) = root.read("DisplayName", JTok::as_string)? { m.DisplayName = v }
        if let Some(v) = root.read("Version", JTok::as_string)? { m.Version = v }
        if let Some(v) = root.read("PublishedFileID", JTok::as_u64)? { m.PublishedFileID = v }
        if let Some(v) = root.read("CreatorUGCIdentity", JTok::as_string)? { m.CreatorUGCIdentity = v }
        let required = |v: &JTok| -> Result<Option<Vec<RequiredMod>>, String> {
            match v {
                JTok::Null | JTok::Undefined => Ok(None),
                JTok::Array(items) => items.iter().map(|it| match it {
                    JTok::Object(_) => Ok(RequiredMod {
                        Name: it.read("Name", JTok::as_string)?.flatten(),
                        Identifier: it.read("Identifier", JTok::as_string)?.flatten(),
                        WorkshopId: it.read("WorkshopId", JTok::as_string)?.flatten(),
                    }),
                    _ => Err("Error converting value to type 'RequiredMod'".to_string()),
                }).collect::<Result<Vec<_>, _>>().map(Some),
                _ => Err("Cannot deserialize the JSON value into type 'List`1[RequiredMod]'".into()),
            }
        };
        if let Some(v) = root.read("RequiredMods", required)? { m.RequiredMods = v }
        Ok(Some(m))
    }

    /// ContraptionMetaData.GetSizeOnDisk
    pub fn GetSizeOnDisk(&self) -> u64 {
        [&self.PathToMetadata, &self.PathToDataFile, &self.PathToThumbnail, &self.PathToOutlineFile]
            .into_iter()
            .flatten()
            .filter_map(|p| std::fs::metadata(p).ok().filter(|m| m.is_file()).map(|m| m.len()))
            .sum()
    }

    /// ContraptionMetaData.IsLegacyStructure
    pub fn IsLegacyStructure(&self, save_directory: &Path) -> bool {
        get_file(save_directory, &format!("{}.json", self.name())).is_file()
    }

    /// ContraptionMetaData.GetPath
    /// A null name reads as "" in `name + ".json"`, and GetSubDir(null) throws ArgumentNullException (Path.Combine).
    pub fn GetPath(save_directory: &Path, name: Option<&str>) -> Result<PathBuf, &'static str> {
        let file = get_file(save_directory, &format!("{}.json", name.unwrap_or("")));
        if file.is_file() {
            return Ok(file);
        }
        let name = name.ok_or("ArgumentNullException")?;
        Ok(get_file(&get_sub_dir(save_directory, name), &format!("{name}.json")))
    }

    pub(crate) fn set_paths(&mut self, directory: &Path, file_stem: &str) {
        self.Directory = Some(directory.to_path_buf());
        self.PathToMetadata = Some(get_file(directory, &format!("{file_stem}.json")));
        self.PathToDataFile = Some(get_file(directory, &format!("{file_stem}.jaap")));
        self.PathToThumbnail = Some(get_file(directory, &format!("{file_stem}.png")));
        self.PathToOutlineFile = Some(get_file(directory, &format!("{file_stem}.outline")));
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// ContraptionSerialiser
// ---------------------------------------------------------------------------------------------------------------------------------

/// ContraptionSerialiser.SaveDirectory (relative to the working directory)
pub fn save_directory() -> PathBuf {
    PathBuf::from("Contraptions")
}

/// PathExtensions.GetFile: Path.Combine(dir.FullName, fileName) (a rooted fileName replaces the directory, as Path.Combine does).
pub fn get_file(dir: &Path, file_name: &str) -> PathBuf {
    dir.join(file_name)
}

/// PathExtensions.GetSubDir: Path.Combine(dir.FullName, directoryName).
pub fn get_sub_dir(dir: &Path, directory_name: &str) -> PathBuf {
    dir.join(directory_name)
}

/// ContraptionSerialiser.SaveContraption: the `.jaap` and `.json` files under `<save_directory>/<name>/`. The mods the states need are not listed (mods are out of scope).
/// An existing metadata file keeps its CreatorUGCIdentity.
pub fn save_contraption(save_directory: &Path, name: &str, states: &[ObjectState]) -> Result<ContraptionMetaData, String> {
    let mut metadata = ContraptionMetaData::new(name);
    let directory = get_sub_dir(save_directory, name);
    metadata.set_paths(&directory, name);
    let (data, meta) = (metadata.PathToDataFile.clone().unwrap(), metadata.PathToMetadata.clone().unwrap());
    if !save_directory.exists() {
        std::fs::create_dir_all(save_directory).map_err(|e| e.to_string())?;
    }
    if !directory.exists() {
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    }
    if meta.exists() {
        match std::fs::read_to_string(&meta).map_err(|e| e.to_string()).and_then(|t| ContraptionMetaData::from_json(&t)) {
            Ok(Some(old)) => metadata.CreatorUGCIdentity = old.CreatorUGCIdentity,
            // Debug.LogError("Existing metadata file malformed and will be ignored")
            _ => {}
        }
    }
    // the data file is created (emptied) before the states are serialised
    std::fs::File::create(&data).map_err(|e| e.to_string())?;
    let bytes = serialise_states(states)?;
    std::fs::write(&data, bytes).map_err(|e| e.to_string())?;
    std::fs::write(&meta, metadata.to_json()).map_err(|e| e.to_string())?;
    super::stats::with_stats(|s| s.increment("CONTRAPTIONS_SAVED", 1.0));
    Ok(metadata)
}

/// What ContraptionSerialiser.LoadContraption shows: NotificationControllerBehaviour.Show texts and DialogBoxManager.Notification texts,
/// and its Debug.LogWarning texts.
#[derive(Debug, Default)]
pub struct LoadMessages {
    pub notifications: Vec<String>,
    pub dialogs: Vec<String>,
    pub warnings: Vec<String>,
}

/// `ModdingApi.FindSpawnable(name)?.MigrationEvents` by name, each event with the name it replaces its spawnable with (no mod adds a spawnable);
/// filled with the catalog's lookups (Game::get_spawnable).
pub static MIGRATIONS: std::sync::OnceLock<std::collections::HashMap<String, Vec<(MigrationEvent, String)>>> = std::sync::OnceLock::new();

/// ContraptionSerialiser.LoadContraption.
pub fn load_contraption(metadata: &mut ContraptionMetaData, messages: &mut LoadMessages) -> Result<Vec<ObjectState>, String> {
    if metadata.Version.as_deref() != Some(current_version()) {
        messages.notifications.push(crate::il_v!(str "ContraptionSerialiser::LoadContraption" 1, "ContraptionSerialiser::LoadContraption" 0).into());
        messages.warnings.push(format!("Attempt to load save from {} into {}", metadata.Version.as_deref().unwrap_or(""), current_version()));
    }
    let path = metadata.PathToDataFile.clone().ok_or("no data file")?;
    let buffer = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut states = match deserialise_states(&buffer) {
        Ok(s) => s,
        Err(_) => {
            // Debug.LogException, UISoundBehaviour.Main.Error(), then Initialise() (a new serializer)
            messages.warnings.push(format!("Error deserialising {}", metadata.Name.as_deref().unwrap_or("")));
            messages.dialogs.push(format!("\"{}\" is corrupted", metadata.name()));
            return Ok(Vec::new());
        }
    };
    if metadata.DisplayName.as_deref().map_or(true, |s| s.trim().is_empty()) {
        metadata.DisplayName = metadata.Name.clone();
    }
    // ModdingApi.FindSpawnable reads the catalog CatalogBehaviour filled at startup: built here when no lookup did it yet.
    if MIGRATIONS.get().is_none() && !states.is_empty() {
        let game = crate::game_assets::find_game(None)?;
        crate::game::Game::build_spawnable_migrations(&mut crate::assets::db::AssetDb::open(&game)?)?;
    }
    for state in &mut states {
        for (event, to) in MIGRATIONS.get().and_then(|m| m.get(&state.SpawnableAssetName)).cloned().unwrap_or_default() {
            if event.IsApplicable(metadata.Version.as_deref().unwrap_or(""), true) {
                state.SpawnableAssetName = to;
            }
        }
    }
    Ok(states)
}

/// ContraptionSerialiser.LoadThumbnail: the decoded picture (RGBA, bottom row first like a Unity texture), or None where the game shows
/// the `Sprites/ErrorWhileLoadingThumbnail` resource.
pub fn load_thumbnail(meta: &ContraptionMetaData) -> Option<Thumbnail> {
    let path = meta.PathToThumbnail.as_ref()?;
    // Debug.LogWarning("Error while loading thumbnail ...") on a failure
    decode_png(&std::fs::read(path).ok()?).ok()
}

/// Texture2D.LoadImage: PNG or JPEG bytes (by their signature) as RGBA, bottom row first like a Unity texture; a JPEG has
/// no alpha (RGB24). Unchecked: the JPEG decoder is zune-jpeg, not Unity's, so a pixel may round one level apart.
pub(crate) fn decode_png(bytes: &[u8]) -> Result<Thumbnail, String> {
        if bytes.starts_with(&[0xff, 0xd8]) {
            use zune_jpeg::zune_core::{colorspace::ColorSpace, options::DecoderOptions};
            let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
            let mut decoder = zune_jpeg::JpegDecoder::new_with_options(std::io::Cursor::new(bytes), options);
            let top_first = decoder.decode().map_err(|e| e.to_string())?;
            let info = decoder.info().ok_or("no JPEG header")?;
            let (w, h) = (info.width as usize, info.height as usize);
            let rgba = top_first.chunks(w * 4).rev().flat_map(|row| row.chunks(4).flat_map(|p| [p[0], p[1], p[2], 255])).collect();
            return Ok(Thumbnail { width: w, height: h, rgba });
        }
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
        let mut buf = vec![0; reader.output_buffer_size().ok_or("too large")?];
        let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
        let (w, h) = (info.width as usize, info.height as usize);
        let channels = info.color_type.samples();
        let mut rgba = Vec::with_capacity(w * h * 4);
        for y in (0..h).rev() {
            for x in 0..w {
                let p = &buf[(y * w + x) * channels..][..channels];
                rgba.extend_from_slice(&match channels {
                    1 => [p[0], p[0], p[0], 255],
                    2 => [p[0], p[0], p[0], p[1]],
                    3 => [p[0], p[1], p[2], 255],
                    _ => [p[0], p[1], p[2], p[3]],
                });
            }
        }
        Ok(Thumbnail { width: w, height: h, rgba })
}

/// A Texture2D a mod loads from a file: its pixels, filterMode (Point or Bilinear) and wrapMode Clamp (the only one set).
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedTexture {
    pub image: Thumbnail,
    pub point_filter: bool,
}

/// A Sprite.Create(texture, Rect(0, 0, width, height), 0.5 * Vector2.one, pixelsPerUnit).
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedSprite {
    pub texture: LoadedTexture,
    pub rect: [f32; 4],
    pub pivot: [f32; 2],
    pub pixels_per_unit: f32,
}

/// Utils.LoadTexture (fullPath, mode, markNonReadable): File.ReadAllBytes, whose failure is Debug.LogError and null; new
/// Texture2D(0, 0).LoadImage(data, markNonReadable) false throws InvalidDataException; then filterMode = mode and wrapMode =
/// Clamp. (markNonReadable only frees the CPU copy: nothing reads it back.)
pub fn load_texture(full_path: &Path, point_filter: bool) -> Option<LoadedTexture> {
    let data = match std::fs::read(full_path) {
        Ok(d) => d,
        // catch (Exception message): Debug.LogError(message), console only.
        Err(_) => return None,
    };
    let Ok(image) = decode_png(&data) else {
        crate::game::menu::throw(format!("InvalidDataException: Texture at {} cannot be loaded as a PNG", full_path.display()))
    };
    Some(LoadedTexture { image, point_filter })
}

/// Utils.LoadSprite (fullPath, mode, pixelsPerUnit, markNonReadable): LoadTexture, null stays null; else Sprite.Create over the
/// whole texture, pivot (0.5, 0.5).
pub fn load_sprite(full_path: &Path, point_filter: bool, pixels_per_unit: f32) -> Option<LoadedSprite> {
    let texture = load_texture(full_path, point_filter)?;
    let rect = [0.0, 0.0, texture.image.width as f32, texture.image.height as f32];
    Some(LoadedSprite { texture, rect, pivot: [0.5, 0.5], pixels_per_unit })
}

/// ContraptionSerialiser.SaveThumbnail: the PNG bytes ThumbnailCreator.CreateThumbnail(states, 512) renders come from the caller (rendering is the UI's).
pub fn save_thumbnail(metadata: &ContraptionMetaData, png_bytes: &[u8]) -> Result<(), String> {
    std::fs::write(metadata.PathToThumbnail.as_ref().ok_or("no thumbnail path")?, png_bytes).map_err(|e| e.to_string())
}

/// A loaded thumbnail (Texture2D).
#[derive(Clone, Debug, PartialEq)]
pub struct Thumbnail {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// ContraptionSerialiser.ConvertToDirectoryBased (obsolete): moves a legacy `<name>.json/.png/.jaap` into its own folder.
pub fn convert_to_directory_based(save_directory: &Path, contraption: &ContraptionMetaData) -> Result<(), String> {
    if contraption.IsLegacyStructure(save_directory) {
        let name = contraption.name();
        let dir = save_directory.join(name);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for ext in ["json", "png", "jaap"] {
            std::fs::rename(save_directory.join(format!("{name}.{ext}")), dir.join(format!("{name}.{ext}"))).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------------------
// ContraptionFolderSource
// ---------------------------------------------------------------------------------------------------------------------------------

/// ContraptionFolderSource
#[derive(Debug, Default)]
pub struct ContraptionFolderSource {
    pub Folder: PathBuf,
}

impl ContraptionFolderSource {
    /// ContraptionFolderSource.constructor: the folder is created when missing.
    pub fn new(source: &Path) -> Self {
        if !source.exists() {
            let _ = std::fs::create_dir_all(source);
        }
        ContraptionFolderSource { Folder: source.to_path_buf() }
    }

    /// ContraptionFolderSource.GetContraptions: every sub-directory with a `.json` file (the first one) whose metadata parses. A directory listing has no defined order (the file system's).
    pub fn GetContraptions(&self) -> Vec<ContraptionMetaData> {
        let mut out = Vec::new();
        let Ok(dirs) = std::fs::read_dir(&self.Folder) else { return out };
        for item in dirs.flatten().filter(|e| e.path().is_dir()) {
            let item = item.path();
            let Some(json) = std::fs::read_dir(&item).into_iter().flatten().flatten().map(|e| e.path()).find(|p| p.is_file() && p.extension().map_or(false, |x| x.eq_ignore_ascii_case("json"))) else { continue };
            let stem = json.file_stem().unwrap().to_string_lossy().to_string();
            // Debug.LogWarning("Failed to read contraption at ...") on an exception
            let Ok(Some(mut m)) = super::jsonnet::read_all_text(&json).map_err(|e| e.to_string()).and_then(|t| ContraptionMetaData::from_json(&t)) else { continue };
            m.set_paths(&item, &stem);
            if m.DisplayName.as_deref().map_or(true, |s| s.trim().is_empty()) {
                m.DisplayName = m.Name.clone();
            }
            out.push(m);
        }
        out
    }
}

/// ContraptionSerialiser.GetAllContraptions: every source's contraptions.
pub fn get_all_contraptions(sources: &[ContraptionFolderSource]) -> Vec<ContraptionMetaData> {
    sources.iter().flat_map(|s| s.GetContraptions()).collect()
}

// ---------------------------------------------------------------------------------------------------------------------------------
// PollingDirectoryWatcher
// ---------------------------------------------------------------------------------------------------------------------------------

/// Ceras xxHash.CalculateHash (xxHash32 of the whole buffer with this seed).
pub fn xxhash(buf: &[u8], seed: u32) -> u32 {
    const P1: u32 = 2654435761;
    const P2: u32 = 2246822519;
    const P3: u32 = 3266489917;
    const P4: u32 = 668265263;
    const P5: u32 = 374761393;
    let len = buf.len();
    let rd = |i: usize| u32::from_le_bytes([buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]);
    let sub = |v: u32, i: usize| (v.wrapping_add(rd(i).wrapping_mul(P2))).rotate_left(13).wrapping_mul(P1);
    let mut index = 0;
    let mut h32;
    if len >= 16 {
        let limit = len - 16;
        let (mut v1, mut v2, mut v3, mut v4) = (seed.wrapping_add(P1).wrapping_add(P2), seed.wrapping_add(P2), seed, seed.wrapping_sub(P1));
        loop {
            v1 = sub(v1, index);
            v2 = sub(v2, index + 4);
            v3 = sub(v3, index + 8);
            v4 = sub(v4, index + 12);
            index += 16;
            if index > limit {
                break;
            }
        }
        h32 = v1.rotate_left(1).wrapping_add(v2.rotate_left(7)).wrapping_add(v3.rotate_left(12)).wrapping_add(v4.rotate_left(18));
    } else {
        h32 = seed.wrapping_add(P5);
    }
    h32 = h32.wrapping_add(len as u32);
    while index + 4 <= len {
        h32 = h32.wrapping_add(rd(index).wrapping_mul(P3)).rotate_left(17).wrapping_mul(P4);
        index += 4;
    }
    while index < len {
        h32 = h32.wrapping_add((buf[index] as u32).wrapping_mul(P5)).rotate_left(11).wrapping_mul(P1);
        index += 1;
    }
    h32 ^= h32 >> 15;
    h32 = h32.wrapping_mul(P2);
    h32 ^= h32 >> 13;
    h32 = h32.wrapping_mul(P3);
    h32 ^= h32 >> 16;
    h32
}

/// PollingDirectoryWatcher
#[derive(Default)]
pub struct PollingDirectoryWatcher {
    pub Interval: std::time::Duration,
    directory: PathBuf,
    pattern: String,
    pub(crate) isWatching: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The OnChange delegate's invocation list (empty is null), shared with the watch thread, which reads it each pass.
    OnChange: std::sync::Arc<std::sync::Mutex<Vec<WatcherHandler>>>,
}

/// An Action subscribed to PollingDirectoryWatcher.OnChange (compared by identity, as a delegate is).
pub type WatcherHandler = std::sync::Arc<dyn Fn() + Send + Sync>;

/// Its fields; OnChange as its subscriber count.
impl std::fmt::Debug for PollingDirectoryWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PollingDirectoryWatcher")
            .field("Interval", &self.Interval)
            .field("directory", &self.directory)
            .field("pattern", &self.pattern)
            .field("isWatching", &self.isWatching)
            .field("OnChange", &self.OnChange.lock().unwrap().len())
            .finish()
    }
}

fn files_below(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            files_below(&p, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

fn pattern_matches(pattern: &str, name: &str) -> bool {
    match pattern.strip_prefix('*') {
        Some("") => true,
        Some(rest) => name.to_lowercase().ends_with(&rest.to_lowercase()),
        None => name.eq_ignore_ascii_case(pattern),
    }
}

impl PollingDirectoryWatcher {
    /// PollingDirectoryWatcher.constructor (Interval defaults to five minutes)
    pub fn new(directory: &Path, pattern: &str) -> Self {
        PollingDirectoryWatcher {
            Interval: std::time::Duration::from_secs(300),
            directory: directory.to_path_buf(),
            pattern: pattern.to_string(),
            isWatching: Default::default(),
            OnChange: Default::default(),
        }
    }

    /// PollingDirectoryWatcher.add_OnChange: Delegate.Combine, the handler appended to the invocation list.
    pub fn add_OnChange(&self, h: WatcherHandler) {
        self.OnChange.lock().unwrap().push(h);
    }

    /// PollingDirectoryWatcher.remove_OnChange: Delegate.Remove, the last subscription of that handler out of the list
    /// (none: unchanged).
    pub fn remove_OnChange(&self, h: &WatcherHandler) {
        let mut list = self.OnChange.lock().unwrap();
        if let Some(i) = list.iter().rposition(|x| std::sync::Arc::ptr_eq(x, h)) {
            list.remove(i);
        }
    }

    /// OnChange?.Invoke(): each handler in the order added.
    pub(crate) fn invoke_on_change(&self) {
        let list = self.OnChange.lock().unwrap().clone();
        for f in list {
            f();
        }
    }

    /// One pass of WatchLoop's body: the XOR of every matching file's hash, files ordered by full name (ordinal here; any fixed order tells a change), the hash seeded with the file's index.
    pub fn directory_hash(&self) -> std::io::Result<u64> {
        let mut files = Vec::new();
        files_below(&self.directory, &mut files)?;
        files.retain(|p| p.file_name().map_or(false, |n| pattern_matches(&self.pattern, &n.to_string_lossy())));
        files.sort();
        let mut hash = 0u64;
        for (i, f) in files.iter().enumerate() {
            hash ^= xxhash(&std::fs::read(f)?, i as u32) as u64;
        }
        Ok(hash)
    }

    /// PollingDirectoryWatcher.Start: the loop runs on its own thread until Stop.
    pub fn Start(&self) {
        self.isWatching.store(true, std::sync::atomic::Ordering::SeqCst);
        let me = PollingDirectoryWatcher { Interval: self.Interval, directory: self.directory.clone(), pattern: self.pattern.clone(), isWatching: self.isWatching.clone(), OnChange: self.OnChange.clone() };
        std::thread::spawn(move || me.WatchLoop());
    }

    /// The directory, pattern and isWatching (the fn-check twins read them).
    pub(crate) fn watch_state(&self) -> (&Path, &str, bool) {
        (&self.directory, &self.pattern, self.isWatching.load(std::sync::atomic::Ordering::SeqCst))
    }
    /// PollingDirectoryWatcher.Stop and PollingDirectoryWatcher.Dispose
    pub fn Stop(&self) {
        self.isWatching.store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// PollingDirectoryWatcher.WatchLoop
    fn WatchLoop(&self) {
        let mut last = 0u64;
        while self.isWatching.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(self.Interval);
            if !self.directory.exists() {
                continue;
            }
            // Debug.LogWarning(exception) on a failure
            if let Ok(hash) = self.directory_hash() {
                if hash != last {
                    last = hash;
                    self.invoke_on_change();
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// ContraptionSpriteStorage
// ---------------------------------------------------------------------------------------------------------------------------------

/// ContraptionSpriteStorage: the sprites (the thumbnails' pictures) by thumbnail path; None is a path whose sprite failed to build.
#[derive(Default)]
pub struct ContraptionSpriteStorage {
    pub Sprites: HashMap<PathBuf, Option<Thumbnail>>,
}

impl ContraptionSpriteStorage {
    /// ContraptionSpriteStorage.Clear
    pub fn Clear(&mut self) {
        self.Sprites.clear();
    }

    /// ContraptionSpriteStorage.HasFor
    pub fn HasFor(&self, c: &ContraptionMetaData) -> bool {
        c.PathToThumbnail.as_ref().map_or(false, |p| self.Sprites.contains_key(p))
    }

    /// ContraptionSpriteStorage.GetFor: the stored sprite, else the thumbnail loaded and stored.
    pub fn GetFor(&mut self, c: &ContraptionMetaData) -> Option<Thumbnail> {
        let Some(path) = c.PathToThumbnail.clone() else { return None };
        if let Some(v) = self.Sprites.get(&path) {
            return v.clone();
        }
        // LoadThumbnail's error picture is the resource sprite; a failed decode is stored as null by the catch
        let t = load_thumbnail(c);
        self.SetFor(c, t.clone());
        t
    }

    /// ContraptionSpriteStorage.SetFor
    pub fn SetFor(&mut self, c: &ContraptionMetaData, s: Option<Thumbnail>) {
        if let Some(p) = &c.PathToThumbnail {
            self.Sprites.insert(p.clone(), s);
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// ContraptionOutlineSerialiser
// ---------------------------------------------------------------------------------------------------------------------------------

/// RotatedRectangle
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RotatedRectangle {
    pub Center: [f32; 2],
    pub Size: [f32; 2],
    pub AngleDegrees: f32,
}

/// One PhysicalBehaviour with a sprite, as GenerateOutline reads it.
#[derive(Clone, Copy, Debug)]
pub struct OutlinePart {
    pub position: [f32; 3],
    pub lossy_scale: [f32; 3],
    pub sprite_size: [f32; 3],
    pub euler_z: f32,
}

/// A selected object's root: whether the selected PhysicalBehaviour has a SerialisableIdentity, the root's instance id, and the PhysicalBehaviours with a spriteRenderer among
/// `root.EnumerateChildrenAndSelf()`, in that order.
#[derive(Clone, Debug)]
pub struct OutlineRoot {
    pub has_identity: bool,
    pub root_id: i32,
    pub parts: Vec<OutlinePart>,
}

/// ContraptionOutlineSerialiser.GenerateOutline
pub fn generate_outline(roots: &[OutlineRoot], center: [f32; 3]) -> Vec<RotatedRectangle> {
    // ContraptionOutline..ctor(rectangles): Rectangles = rectangles. The struct is its array, so the list stands for it.
    let mut list = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for r in roots {
        if !r.has_identity || !seen.insert(r.root_id) {
            continue;
        }
        for p in &r.parts {
            let size2 = [p.sprite_size[0] * p.lossy_scale[0], p.sprite_size[1] * p.lossy_scale[1]];
            let v = [p.position[0] - center[0], p.position[1] - center[1]];
            list.push(RotatedRectangle { Center: v, Size: size2, AngleDegrees: p.euler_z });
        }
    }
    list
}

/// ContraptionOutlineSerialiser.SaveOutline's `round`: Mathf.Round(x * 100f) / 100f (half to even like Mathf.Round).
pub(crate) fn outline_round(x: f32) -> f32 {
    (x * 100.0).round_ties_even() / 100.0
}

/// A float as .NET's invariant `float.ToString()` (shortest round-trip digits; "E" form outside 1e-5..1e15).
fn dotnet_float(x: f32) -> String {
    if x == 0.0 {
        return if x.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let a = x.abs();
    if a >= 1e15 || a < 1e-5 {
        let s = format!("{x:e}");
        let (m, e) = s.split_once('e').unwrap();
        let e: i32 = e.parse().unwrap();
        return format!("{m}E{}{:02}", if e < 0 { "-" } else { "+" }, e.abs());
    }
    format!("{x}")
}

/// ContraptionOutlineSerialiser.SaveOutline
pub fn save_outline(metadata: &ContraptionMetaData, rectangles: &[RotatedRectangle]) -> Result<(), String> {
    // WriteLine("1.28"): Environment.NewLine, "\r\n" on the Windows build (fnb1r12b's olw=); each rectangle ends in Write('\n').
    // 1.28.45: WriteLine(GameInfo.Version).
    let mut text = format!("{}\r\n", current_version());
    for r in rectangles {
        let v = [r.Center[0], r.Center[1], r.Size[0], r.Size[1], r.AngleDegrees];
        text.push_str(&v.iter().map(|&x| dotnet_float(outline_round(x))).collect::<Vec<_>>().join(" "));
        text.push('\n');
    }
    std::fs::write(metadata.PathToOutlineFile.as_ref().ok_or("no outline path")?, text).map_err(|e| e.to_string())
}

/// ContraptionOutlineSerialiser.LoadOutline: a malformed line stops the read (the rectangles before it are kept).
pub fn load_outline(path: &Path) -> Result<Vec<RotatedRectangle>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut list = Vec::new();
    for (i, line) in text.lines().enumerate().skip(1) {
        let parts: Vec<String> = line.replace(',', ".").split(' ').map(String::from).collect();
        // Debug.LogErrorFormat("Failed to deserialise outline: {0}", ex) on a malformed line
        if parts.len() < 5 {
            break;
        }
        let f: Result<Vec<f32>, _> = parts[..5].iter().map(|s| s.as_str().parse::<f32>()).collect();
        let Ok(f) = f else { break };
        let _ = i;
        list.push(RotatedRectangle { Center: [f[0], f[1]], Size: [f[2], f[3]], AngleDegrees: f[4] });
    }
    Ok(list)
}

// ---------------------------------------------------------------------------------------------------------------------------------
// MigrationEvent
// ---------------------------------------------------------------------------------------------------------------------------------

/// MigrationEvent.Condition
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum MigrationCondition {
    #[default]
    OlderThan,
    OlderThanOrEqualTo,
    EqualTo,
    NewerThan,
    NewerThanOrEqualTo,
}

/// MigrationEvent
#[derive(Clone, Debug)]
pub struct MigrationEvent {
    pub Version: String,
    pub VersionCondition: MigrationCondition,
}

impl Default for MigrationEvent {
    fn default() -> Self {
        Self {
            Version: current_version().to_string(),
            VersionCondition: Default::default(),
        }
    }
}

/// LegacyGameVersionUtils.ParseBuildIndex
pub fn parse_build_index(input: &str) -> Result<i32, String> {
    let input = input.to_lowercase();
    let input = input.trim();
    let at = input.find(crate::il!(str "LegacyGameVersionUtils::ParseBuildIndex" 0)).or_else(|| input.find(crate::il!(str "LegacyGameVersionUtils::ParseBuildIndex" 1)));
    let version = &input[..at.unwrap_or(input.len())];
    let nums: Vec<i32> = version.trim().split('.').map(|p| p.trim().parse::<i32>().map_err(|_| format!("bad version {input}"))).collect::<Result<_, _>>()?;
    // Version.Parse: two to four components, none negative.
    if !(2..=4).contains(&nums.len()) || nums.iter().any(|&n| n < 0) {
        return Err(format!("bad version {input}"));
    }
    let n = |i: usize| nums.get(i).copied().unwrap_or(-1).max(0);
    let mut total = n(0) * 10000 + n(1) * 1000 + n(2) * 100;
    if let Some(at) = at {
        let text = &input[at..];
        let mut result = 1;
        if let Some(d) = text.find(|c: char| c.is_numeric()) {
            match text[d..].trim().parse::<i32>() {
                Ok(v) if v >= 1 => result = v,
                _ => return Ok(i32::MAX),
            }
        }
        total += result;
    } else {
        total += 99;
    }
    Ok(if total != -1 { total } else { i32::MAX })
}

/// The version text the game writes and compares saves by: the literal "1.28" in 1.28.41's code, GameInfo.Version.ToString()
/// (Major.Minor.Build, 1.28.52) from 1.28.45 (the newer build) on.
pub fn current_version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        if crate::assets::il::is_1_28_45() {
            // GameInfo..cctor: new Version(1, 28, 52).
            let (a, b, c) = crate::ui::debug_info::version();
            format!("{a}.{b}.{c}")
        } else {
            "1.28".to_string()
        }
    })
}

/// System.Version.Parse as (Major, Minor, Build, Revision), an unset part -1 (so 1.28 < 1.28.0); None where it throws: other than two to
/// four '.'-separated parts, or a part Int32.TryParse (NumberStyles.Integer: white space around, a leading sign) rejects or below 0.
pub fn version_parse(s: &str) -> Option<(i32, i32, i32, i32)> {
    let parts: Vec<&str> = s.split('.').collect();
    if !(2..=4).contains(&parts.len()) {
        return None;
    }
    let mut v = [-1i32; 4];
    for (i, p) in parts.iter().enumerate() {
        let t = p.trim_matches(|c: char| c == ' ' || ('\u{9}'..='\u{d}').contains(&c));
        let (neg, digits) = match t.as_bytes().first() {
            Some(b'-') => (true, &t[1..]),
            Some(b'+') => (false, &t[1..]),
            _ => (false, t),
        };
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let n: i64 = digits.parse().ok()?;
        let n = if neg { -n } else { n };
        if n < 0 || n > i32::MAX as i64 {
            return None;
        }
        v[i] = n as i32;
    }
    Some((v[0], v[1], v[2], v[3]))
}

/// LegacyGameVersionUtils.GetDifference: ParseBuildIndex(a) - ParseBuildIndex(b).
pub fn legacy_game_version_get_difference(a: &str, b: &str) -> Result<i32, String> {
    Ok(parse_build_index(a)? - parse_build_index(b)?)
}

/// LegacyGameVersionUtils.IsOlderThan: GetDifference(a, b) < 0 (no caller in the game; mods called it).
pub fn legacy_game_version_is_older_than(a: &str, b: &str) -> Result<bool, String> {
    Ok(legacy_game_version_get_difference(a, b)? < 0)
}

/// LegacyGameVersionUtils.IsAtLeast: GetDifference(a, b) >= 0.
pub fn legacy_game_version_is_at_least(a: &str, b: &str) -> Result<bool, String> {
    Ok(legacy_game_version_get_difference(a, b)? >= 0)
}

/// LegacyGameVersionUtils.IsNewerThan: GetDifference(a, b) > 0.
pub fn legacy_game_version_is_newer_than(a: &str, b: &str) -> Result<bool, String> {
    Ok(legacy_game_version_get_difference(a, b)? > 0)
}

impl MigrationEvent {
    /// MigrationEvent.IsApplicable (`has_target` is `ToSpawnInstead != null`)
    pub fn IsApplicable(&self, origin_version: &str, has_target: bool) -> bool {
        if !has_target {
            return false;
        }
        // 1.28.45: a "1.28..." origin compares as System.Version (a parse failure is false).
        if crate::assets::il::is_1_28_45() && origin_version.starts_with("1.28") {
            let (Some(a), Some(b)) = (version_parse(origin_version), version_parse(&self.Version)) else { return false };
            return match self.VersionCondition {
                MigrationCondition::OlderThan => a < b,
                MigrationCondition::OlderThanOrEqualTo => a <= b,
                MigrationCondition::EqualTo => a == b,
                MigrationCondition::NewerThan => a > b,
                MigrationCondition::NewerThanOrEqualTo => a >= b,
            };
        }
        let Ok(difference) = legacy_game_version_get_difference(origin_version, &self.Version) else { return false };
        match self.VersionCondition {
            MigrationCondition::OlderThan => difference < 0,
            MigrationCondition::OlderThanOrEqualTo => difference <= 0,
            MigrationCondition::EqualTo => difference == 0,
            MigrationCondition::NewerThan => difference > 0,
            MigrationCondition::NewerThanOrEqualTo => difference >= 0,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// ContraptionSavePoint, ContextMenuBehaviour.SaveAction, DeleteContraptionButtonBehaviour
// ---------------------------------------------------------------------------------------------------------------------------------

/// ContraptionSavePoint
#[derive(Clone, Debug, Default)]
pub struct ContraptionSavePoint {
    /// The selected PhysicalBehaviours (physical indices).
    pub GameObjectsToSave: Vec<usize>,
    pub NameToSave: String,
    pub NewOrigin: [f32; 3],
    pub(super) lockName: bool,
}

impl ContraptionSavePoint {
    /// ContraptionSavePoint.SetName
    pub fn SetName(&mut self, name: &str) {
        if !self.lockName {
            self.NameToSave = name.to_string();
        }
    }
}

/// What ContraptionSavePoint.Serialise leaves for the UI: the notification, and the sound.
#[derive(Debug, Default)]
pub struct SaveReport {
    pub notification: String,
    pub metadata: Option<ContraptionMetaData>,
}

/// ThumbnailCreator.CreateThumbnail(states, 512) waiting to be drawn: the `.png` to write, the physicals whose sprites it shows (the saved objects and their hierarchy),
/// the camera's centre and ThumbnailCreator.TakePicture's orthographicSize * 2 (`thumbnail_framing`).
#[derive(Clone, Debug, PartialEq)]
pub struct ThumbnailJob {
    pub path: PathBuf,
    pub physicals: Vec<usize>,
    pub centre: [f32; 2],
    pub size: f32,
    /// ThumbnailCreator's fields in the install's scene.
    pub scene: crate::thumbnail::Scene,
}

/// `ContraptionSerialiser.SaveThumbnail`'s resolution.
pub const THUMBNAIL_RESOLUTION: u32 = 512;

/// ThumbnailCreator.TakePicture's framing (1.28.41): the camera's (and Background's) centre and `num` (orthographicSize * 2).
/// One object: its position, and num = the longer side of its SpriteRenderer's bounds + ThumbnailPadding (8 without a
/// SpriteRenderer). More: the box of the positions, each padded by ThumbnailPadding, and num its longer side. The centre
/// is the box's middle (`(positiveInfinity + negativeInfinity) * 0.5`, NaN, for none).
pub fn thumbnail_framing(positions: &[[f32; 2]], sprite_size: Option<[f32; 2]>, padding: f32) -> ([f32; 2], f32) {
    use super::mathf::{max as mmax, min as mmin};
    let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    let mut num = 8.0f32;
    if positions.len() == 1 {
        (lo, hi) = (positions[0], positions[0]);
        if let Some(s) = sprite_size {
            num = mmax(s[0], s[1]) + padding;
        }
    } else {
        for p in positions {
            for i in 0..2 {
                lo[i] = mmin(lo[i], p[i] - padding);
                hi[i] = mmax(hi[i], p[i] + padding);
            }
        }
        num = mmax((lo[0] - hi[0]).abs(), (lo[1] - hi[1]).abs());
    }
    ([(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5], num)
}

/// What a dialog button of DialogBoxManager.Dialog runs (the UI calls `Game::contraption_dialog_action`).
#[derive(Clone, Debug, PartialEq)]
pub enum DialogAction {
    /// ContextMenuBehaviour.SaveAction's save(): the overwrite dialog's Continue.
    SaveContraption,
    /// DeleteContraptionButtonBehaviour.RemoveContraption: the confirmation's Yes.
    RemoveContraption,
}

/// DialogBoxManager.Dialog / DialogBoxManager.Notification: the text and its buttons (text, ClosesDialogBox, delegate), waiting for the UI to show it.
#[derive(Clone, Debug, PartialEq)]
pub struct DialogRequest {
    pub title: String,
    /// DialogBox.SetHeight
    pub height: Option<f32>,
    pub buttons: Vec<(String, bool, Option<DialogAction>)>,
}

impl DialogRequest {
    /// DialogBoxManager.Notification: one OK button.
    pub fn notification(text: String) -> Self {
        DialogRequest { title: text, height: None, buttons: vec![("OK".into(), true, None)] }
    }
}

/// The contraption features' state on the Game: ContraptionSavePoint.Main, the dialogs waiting for the UI, the contraption DeleteContraptionButtonBehaviour will
/// remove if "Yes" is pressed, and whether CatalogBehaviour.CreateItemButtons has to run again.
#[derive(Debug, Default)]
pub struct ContraptionState {
    pub save_point: ContraptionSavePoint,
    pub dialogs: Vec<DialogRequest>,
    pub removing: Option<ContraptionMetaData>,
    pub catalog_dirty: bool,
    /// ContextMenuBehaviour.SaveAction's entered name, between askForSave and save().
    pub pending_name: String,
    /// ContraptionSerialiser.SaveThumbnail's pending pictures: ThumbnailCreator.CreateThumbnail renders them (src/thumbnail.rs) and writes the `.png`.
    pub thumbnails: Vec<ThumbnailJob>,
}

impl Game {
    /// ContraptionOutlineSerialiser.SaveOutline's input: the selected objects' roots with the PhysicalBehaviours that have a spriteRenderer, in
    /// `transform.root.EnumerateChildrenAndSelf()` order, as GenerateOutline reads them.
    pub fn contraption_outline_roots(&mut self, selected: &[usize]) -> Vec<OutlineRoot> {
        let mut roots = Vec::new();
        for &p in selected {
            if self.physicals[p].destroyed {
                continue;
            }
            let root = self.root_id(p);
            let has_identity = self.identities.contains_key(&self.physicals[p].transform_id);
            // The IL tests TryGetComponent<SerialisableIdentity> and HashSet.Add(root id) before it enumerates the root.
            if !has_identity || roots.iter().any(|r: &OutlineRoot| r.has_identity && r.root_id == root) {
                roots.push(OutlineRoot { has_identity, root_id: root, parts: Vec::new() });
                continue;
            }
            // A map object's transform.root is the scene's WORLD (WORLD > MAP > the map clone; WORLD's other scene children,
            // HeatEffect, ColdEffect and Ambience emitter, hold no PhysicalBehaviour and no `transform_nodes` entry): the walk is
            // WORLD's transform nodes, depth first in creation (= sibling) order, with the map clone's own walk at its root.
            let in_clone = self.unsynced.iter().chain(&self.retained).any(|i| i.objects.iter().any(|x| x.components.contains(&root)));
            let map_under = self.map_root.filter(|&m| {
                let mut n = self.transform_nodes.iter().position(|x| x.id == m);
                while let Some(i) = n {
                    if self.transform_nodes[i].id == root { return true }
                    n = self.transform_nodes[i].parent;
                }
                false
            });
            let order = match (in_clone, map_under) {
                (false, Some(m)) => {
                    fn walk(g: &mut Game, n: usize, m: i32) -> Vec<i32> {
                        let id = g.transform_nodes[n].id;
                        if id == m {
                            return g.with_component_instance(m, |g, inst, o| g.enumerate_children_and_self(inst, o));
                        }
                        let mut out = vec![id];
                        for c in (0..g.transform_nodes.len()).filter(|&c| g.transform_nodes[c].parent == Some(n)).collect::<Vec<_>>() {
                            out.extend(walk(g, c, m));
                        }
                        out
                    }
                    match self.transform_nodes.iter().position(|x| x.id == root) {
                        Some(n) => walk(self, n, m),
                        None => Vec::new(),
                    }
                }
                _ => self.with_component_instance(root, |g, inst, _| g.enumerate_children_and_self(inst, 0)),
            };
            let mut parts = Vec::new();
            for tid in order {
                let Some(q) = self.physicals.iter().position(|ph| ph.transform_id == tid && !ph.destroyed) else { continue };
                let (Some((_, extents)), true) = (self.physicals[q].sprite_bounds, self.physicals[q].sprite.is_some()) else { continue };
                parts.push(OutlinePart {
                    position: self.physicals[q].transform.position,
                    lossy_scale: self.lossy_scale(q),
                    sprite_size: [extents[0] * 2.0, extents[1] * 2.0, extents[2] * 2.0],
                    euler_z: super::dump::euler_z(self.physicals[q].transform.rotation),
                });
            }
            roots.push(OutlineRoot { has_identity, root_id: root, parts });
        }
        roots
    }

    /// ThumbnailCreator.CreateThumbnail's job for the saved objects: gameObjects are the converted roots (one per distinct spawned root,
    /// ObjectStateConverter.Convert), so the framing reads the roots' positions (relative to the save origin, which ThumbnailCreator's
    /// Center stands for) and, for a single root, its own SpriteRenderer's bounds; the picture shows the whole hierarchy.
    pub fn thumbnail_job(&mut self, selected: &[usize], origin: [f32; 3], path: PathBuf) -> Result<Option<ThumbnailJob>, String> {
        // Without the install's scene there is no ThumbnailCreator to draw with.
        let Some(db) = self.assets.as_deref_mut() else { return Ok(None) };
        let scene = crate::thumbnail::read_scene(db).ok_or("the scene has no ThumbnailCreator")?;
        let mut roots: Vec<usize> = Vec::new();
        for &p in selected {
            let r = self.root_id(p);
            if !self.physicals[p].destroyed && self.object_names.contains_key(&r) && !roots.iter().any(|&q| self.root_id(q) == r) {
                roots.push(p);
            }
        }
        let mut positions = Vec::new();
        for &p in &roots {
            let t = match self.root_of(p) {
                (true, n) => self.node_world(n).0,
                (false, q) => self.physicals[q].transform.position,
            };
            positions.push([t[0] - origin[0], t[1] - origin[1]]);
        }
        // TryGetComponent<SpriteRenderer> on the single root: its bounds' size.
        let sprite_size = match roots[..] {
            [p] => match self.root_of(p) {
                (false, q) => self.physicals[q].sprite.is_some().then(|| self.sprite_world_bounds(q).1),
                (true, n) => {
                    let comps = self.with_component_instance(self.transform_nodes[n].id, |_, inst, o| inst.objects[o].components.clone());
                    self.renderers.iter().find(|r| comps.contains(&r.id)).map(|r| crate::unity::transform::transform_aabb(r.bounds.0, r.bounds.1, &self.anchored_nodes(r.anchor, &r.chain)).1)
                }
            }
            .map(|e| [e[0] * 2.0, e[1] * 2.0]),
            _ => None,
        };
        let (centre, size) = thumbnail_framing(&positions, sprite_size, scene.padding);
        // The per-function checks (--ui-screens): TakePicture's camera at Center + the framing, and orthographicSize (F4).
        let f4 = |v: f32| if v.is_nan() { "NaN".to_string() } else if v.is_infinite() { (if v < 0.0 { "-Infinity" } else { "Infinity" }).to_string() } else { format!("{v:.4}") };
        crate::ui::uifn("ThumbnailCreator.TakePicture", || format!("{},{} {}", f4(scene.center[0] + centre[0]), f4(scene.center[1] + centre[1]), f4(size * 0.5)));
        let root_ids: Vec<i32> = roots.iter().map(|&p| self.root_id(p)).collect();
        let physicals = (0..self.physicals.len()).filter(|&q| !self.physicals[q].destroyed && root_ids.contains(&self.root_id(q))).collect();
        Ok(Some(ThumbnailJob { path, physicals, centre: [origin[0] + centre[0], origin[1] + centre[1]], size, scene }))
    }

    /// ContraptionSavePoint.DoSave: the name is locked while Serialise runs.
    pub fn contraption_do_save(&mut self) -> SaveReport {
        self.contraption.save_point.lockName = true;
        self.contraption_serialise()
    }

    /// ContraptionSavePoint.Serialise: an empty name becomes "Contraption <1000..9998>"; Convert, SaveContraption, SaveThumbnail, SaveOutline; the notification says whether it
    /// worked, then CatalogBehaviour.CreateItemButtons is asked for.
    fn contraption_serialise(&mut self) -> SaveReport {
        if self.contraption.save_point.NameToSave.trim().is_empty() || self.contraption.save_point.NameToSave.is_empty() {
            self.contraption.save_point.NameToSave = format!("Contraption {}", self.random.range_int(1000, 9999));
        }
        let name = self.contraption.save_point.NameToSave.clone();
        let (selected, origin) = (self.contraption.save_point.GameObjectsToSave.clone(), self.contraption.save_point.NewOrigin);
        let mut report = SaveReport::default();
        let pending = self.contraption.thumbnails.len();
        let result = (|| -> Result<ContraptionMetaData, String> {
            let states = self.object_states_of(&selected, origin);
            let metadata = save_contraption(&save_directory(), &name, &states)?;
            let job = self.thumbnail_job(&selected, origin, metadata.PathToThumbnail.clone().ok_or("no thumbnail path")?)?;
            self.contraption.thumbnails.extend(job);
            let roots = self.contraption_outline_roots(&selected);
            save_outline(&metadata, &generate_outline(&roots, origin))?;
            Ok(metadata)
        })();
        match result {
            Ok(m) => {
                // 1.28.45 spells it "successfully".
                report.notification = if crate::assets::il::is_1_28_45() { format!("<b>{name}</b> saved successfully") } else { format!("<b>{name}</b> saved succesfully") };
                report.metadata = Some(m);
            }
            // Debug.LogException(exception)
            Err(_) => report.notification = format!("<b>{name}</b> saved with errors"),
        }
        self.contraption.save_point.lockName = false;
        self.notify(report.notification.clone());
        // CatalogBehaviour.CreateItemButtons runs after SaveThumbnail has written the picture: with a thumbnail still to be rendered the refresh waits for it (thumbnail.rs).
        if self.contraption.thumbnails.len() == pending {
            self.contraption.catalog_dirty = true;
        }
        report
    }

    /// ContextMenuBehaviour.SaveAction (menu open and HasObject): the save point takes the selection and `Camera.main.ScreenToWorldPoint(menu position)` (`at`, its x and y), then the TextEntry
    /// "Enter contraption name" opens; the menu hides.
    pub fn context_save_action(&mut self, at: [f32; 2]) -> bool {
        if !self.context_has_object() {
            return false;
        }
        let selected = self.context_selection();
        save_action_open(&mut self.contraption.save_point, selected, [at[0], at[1], 0.0]);
        self.entry = Some(super::menu::EntryBox { number: false, title: crate::il!(str "ContextMenuBehaviour::SaveAction" 1).into(), placeholder: String::new(), text: String::new(), targets: vec![super::menu::Entry::SaveContraption] });
        true
    }

    /// ContextMenuBehaviour.SaveAction's askForSave, after the entry box's Save.
    pub fn contraption_ask_for_save(&mut self, entered: &str) {
        self.contraption.pending_name = entered.to_string();
        // ContraptionSerialiser.GetAllContraptions: the folder source, then the Steam source's place (the Workshop folder's items).
        let mut all = get_all_contraptions(&[ContraptionFolderSource::new(&save_directory())]);
        all.extend(crate::game::mods::workshop_contraptions());
        match save_action_ask_for_save(entered, &all) {
            // UISoundBehaviour.Main.Error()
            SaveAsk::Invalid(text) => self.contraption.dialogs.push(DialogRequest::notification(text)),
            // UISoundBehaviour.Main.Warning()
            SaveAsk::ConfirmOverwrite => {
                let title = crate::il!(str "ContextMenuBehaviour/<>c__DisplayClass40_0::<SaveAction>g__askForSave|1" 3);
                crate::ui::uifn("DialogBoxManager.Dialog", || format!("Continue|Cancel {title}"));
                self.contraption.dialogs.push(DialogRequest {
                    height: None,
                    title: title.into(),
                    buttons: vec![("Continue".into(), true, Some(DialogAction::SaveContraption)), ("Cancel".into(), true, None)],
                })
            }
            SaveAsk::Save => self.contraption_save_named(),
        }
    }

    /// ContextMenuBehaviour.SaveAction's save(): the entered name to the save point, then DoSave.
    fn contraption_save_named(&mut self) {
        let name = self.contraption.pending_name.clone();
        // save() reads dialog.EnteredText again.
        crate::ui::uifn("DialogBox.get_EnteredText", || format!("{} {}", name.chars().count(), name));
        save_action_name(&mut self.contraption.save_point, &name);
        self.contraption_do_save();
    }

    /// DeleteContraptionButtonBehaviour.CreateDialogBoxAndRemove: "Yes" runs RemoveContraption, "No" closes.
    pub fn contraption_delete_dialog(&mut self, meta: ContraptionMetaData) {
        let title = delete_question(meta.PublishedFileID, &meta);
        self.contraption.removing = Some(meta);
        self.contraption.dialogs.push(DialogRequest { title, height: None, buttons: vec![("Yes".into(), true, Some(DialogAction::RemoveContraption)), ("No".into(), true, None)] });
    }

    /// A dialog button's delegate.
    pub fn contraption_dialog_action(&mut self, a: &DialogAction) {
        match a {
            DialogAction::SaveContraption => self.contraption_save_named(),
            DialogAction::RemoveContraption => {
                let Some(meta) = self.contraption.removing.take() else { return };
                for text in remove_contraption(&meta) {
                    self.contraption.dialogs.push(DialogRequest::notification(text));
                }
                self.contraption.catalog_dirty = true;
            }
        }
    }
}

/// The outcome of ContextMenuBehaviour.SaveAction's askForSave.
#[derive(Debug, PartialEq)]
pub enum SaveAsk {
    /// UISoundBehaviour.Main.Error and this notification.
    Invalid(String),
    /// UISoundBehaviour.Main.Warning and the "will overwrite" dialog; Continue calls `save`.
    ConfirmOverwrite,
    /// `save()` at once.
    Save,
}

/// ContextMenuBehaviour.SaveAction (the part before the dialog): resets the save point's name and takes the selected objects and the origin.
pub fn save_action_open(sp: &mut ContraptionSavePoint, selected: Vec<usize>, origin: [f32; 3]) {
    sp.NameToSave = String::new();
    sp.GameObjectsToSave = selected;
    sp.NewOrigin = origin;
}

/// ContextMenuBehaviour.SaveAction's askForSave: the entered name is checked, then compared (case-insensitively) with the saved contraptions' names.
pub fn save_action_ask_for_save(entered: &str, existing: &[ContraptionMetaData]) -> SaveAsk {
    const INVALID: &[char] = &['"', '<', '>', '|', ':', '*', '?', '\\', '/'];
    crate::ui::uifn("DialogBox.get_EnteredText", || format!("{} {}", entered.chars().count(), entered));
    if entered.trim().is_empty() || entered.chars().any(|c| (c as u32) < 32 || INVALID.contains(&c)) || entered.contains('.') {
        return SaveAsk::Invalid(format!("\"{entered}\" contains invalid characters"));
    }
    let lower = entered.to_lowercase();
    if existing.iter().any(|c| c.name().to_lowercase() == lower) {
        SaveAsk::ConfirmOverwrite
    } else {
        SaveAsk::Save
    }
}

/// ContextMenuBehaviour.SaveAction's save(): the entered name goes to the save point, then DoSave.
pub fn save_action_name(sp: &mut ContraptionSavePoint, entered: &str) {
    sp.NameToSave = entered.to_string();
}

/// DeleteContraptionButtonBehaviour.CreateDialogBoxAndRemove: the dialog's question.
pub fn delete_question(published_file_id: u64, meta: &ContraptionMetaData) -> String {
    format!("{} {}?", if published_file_id == 0 { crate::il!(str "DeleteContraptionButtonBehaviour::CreateDialogBoxAndRemove" 1) } else { crate::il!(str "DeleteContraptionButtonBehaviour::CreateDialogBoxAndRemove" 0) }, meta.DisplayName.clone().unwrap_or_default())
}

/// DeleteContraptionButtonBehaviour.RemoveContraption: the four files are deleted one by one; each failure gives a "Unable to delete" notification text. (A workshop item's Steam
/// unsubscribe, and the catalogue's CreateItemButtons refresh, are the platform's and the UI's.)
pub fn remove_contraption(meta: &ContraptionMetaData) -> Vec<String> {
    let mut dialogs = Vec::new();
    // A Workshop-folder item: the game unsubscribes (Item.GetAsync, nothing without Steam) and deletes its files; the
    // rewrite never writes into the Workshop folder, so they stay.
    if meta.Directory.as_deref().map_or(false, super::mods::in_workshop_directory) {
        return dialogs;
    }
    let paths = [&meta.PathToMetadata, &meta.PathToDataFile, &meta.PathToThumbnail, &meta.PathToOutlineFile];
    // Debug.LogWarning("Attempt to delete contraption that doesn't exist: ...") when the metadata or data file is missing
    for p in paths.into_iter().flatten() {
        if p.is_file() {
            if std::fs::remove_file(p).is_err() {
                dialogs.push(format!("Unable to delete\n{}", p.display()));
            }
        }
    }
    dialogs
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn version_parse_follows_system_version() {
        assert_eq!(version_parse("1.28"), Some((1, 28, -1, -1)));
        assert_eq!(version_parse(" 1.28.51 "), Some((1, 28, 51, -1)));
        assert!(version_parse("1.28").unwrap() < version_parse("1.28.0").unwrap());
        assert_eq!(version_parse("1"), None);
        assert_eq!(version_parse("1.-2"), None);
        assert_eq!(version_parse("1.2.3.4.5"), None);
        assert_eq!(version_parse("1.x"), None);
    }

    #[test]
    fn load_texture_and_sprite() {
        let dir = std::env::temp_dir().join(format!("ppg_load_texture_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.png");
        let mut out = Vec::new();
        {
            let mut e = png::Encoder::new(&mut out, 2, 1);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            e.write_header().unwrap().write_image_data(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        }
        std::fs::write(&path, &out).unwrap();
        let s = load_sprite(&path, true, 35.0).unwrap();
        assert_eq!((s.rect, s.pivot, s.pixels_per_unit, s.texture.point_filter), ([0.0, 0.0, 2.0, 1.0], [0.5, 0.5], 35.0, true));
        assert_eq!(s.texture.image.rgba, vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(load_texture(&dir.join("missing.png"), false).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Every `.jaap` in the installed game's Contraptions folder (found through game_assets::find_game); empty, with a note, when the game or the folder is absent.
    pub(crate) fn fixtures() -> Vec<PathBuf> {
        let root = match crate::game_assets::find_game(None) {
            Ok(game) => game.join("Contraptions"),
            Err(e) => {
                println!("skipped: no installed game ({e})");
                return Vec::new();
            }
        };
        let mut v = Vec::new();
        for d in std::fs::read_dir(&root).into_iter().flatten().flatten() {
            v.extend(std::fs::read_dir(d.path()).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |e| e == "jaap")));
        }
        if v.is_empty() {
            println!("skipped: no .jaap files in {}", root.display());
        }
        v.sort();
        v
    }

    /// Real `.jaap` files load into ObjectStates and write back as the same bytes.
    #[test]
    fn real_jaap_files_round_trip_through_object_states() {
        let files = fixtures();
        for f in files {
            let bytes = std::fs::read(&f).unwrap();
            let states = deserialise_states(&bytes).unwrap();
            // An empty contraption is valid: the real 1.28.52 game wrote one as the 3 bytes 02 02 01 (a Contraption whose
            // ObjectStates is an empty array), and its LoadContraption takes it without the "corrupted" dialog, spawning nothing.
            if states.is_empty() {
                assert_eq!(serialise_states(&states).unwrap(), bytes, "{f:?}: the game's empty contraption");
                continue;
            }
            // Like the game, a type the binder can't resolve is written back as "" (see ceras::tests::real_files_round_trip): the write-back is a fixed point.
            let back = serialise_states(&states).unwrap();
            let again = serialise_states(&deserialise_states(&back).unwrap()).unwrap();
            assert!(again == back, "{f:?}: {} bytes written, {} read", again.len(), back.len());
        }
    }

    /// The real 1.28.52 game's empty contraption file (02 02 01, written by a SaveContraption of no states): no states,
    /// and written back byte for byte.
    #[test]
    fn empty_contraption_file_is_valid() {
        let states = deserialise_states(&[2, 2, 1]).unwrap();
        assert!(states.is_empty());
        assert_eq!(serialise_states(&states).unwrap(), vec![2, 2, 1]);
    }

    /// Save, then load from the folder: the same states, the metadata, the delete button.
    #[test]
    fn save_load_scan_delete() {
        let dir = std::env::temp_dir().join(format!("ppg-contraptions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let Some(first) = fixtures().into_iter().next() else { return };
        let bytes = std::fs::read(&first).unwrap();
        let states = deserialise_states(&bytes).unwrap();
        let meta = save_contraption(&dir, "My Test", &states).unwrap();
        assert_eq!(std::fs::read(meta.PathToDataFile.as_ref().unwrap()).unwrap(), serialise_states(&states).unwrap());
        let text = std::fs::read_to_string(meta.PathToMetadata.as_ref().unwrap()).unwrap();
        // 1.28.41 writes "1.28", 1.28.52 its GameInfo version "1.28.52".
        assert_eq!(text, format!(r#"{{"Name":"My Test","DisplayName":"My Test","Version":"{}","PublishedFileID":0,"CreatorUGCIdentity":null,"RequiredMods":[],"IsCurrentVersion":true,"IsWorkshopItem":false,"RequiresMods":false}}"#, current_version()));
        let source = ContraptionFolderSource::new(&dir);
        let all = source.GetContraptions();
        assert_eq!(all.len(), 1);
        let mut m = all.into_iter().next().unwrap();
        assert_eq!(m.Name.as_deref(), Some("My Test"));
        assert!(m.GetSizeOnDisk() > bytes.len() as u64);
        let mut msgs = LoadMessages::default();
        let loaded = load_contraption(&mut m, &mut msgs).unwrap();
        assert_eq!(loaded, states);
        assert!(msgs.notifications.is_empty() && msgs.dialogs.is_empty());
        // the directory watcher's hash changes with the content
        let w = PollingDirectoryWatcher::new(&dir, "*");
        let h = w.directory_hash().unwrap();
        std::fs::write(m.PathToOutlineFile.as_ref().unwrap(), "1.28\n").unwrap();
        assert_ne!(w.directory_hash().unwrap(), h);
        assert!(remove_contraption(&m).is_empty());
        assert!(!m.PathToDataFile.as_ref().unwrap().exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A damaged `.jaap` gives the corrupted dialog and an empty contraption.
    #[test]
    fn corrupted_file() {
        let dir = std::env::temp_dir().join(format!("ppg-contraptions-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut meta = save_contraption(&dir, "Bad", &[]).unwrap();
        std::fs::write(meta.PathToDataFile.as_ref().unwrap(), [1u8, 2, 3]).unwrap();
        let mut msgs = LoadMessages::default();
        assert!(load_contraption(&mut meta, &mut msgs).unwrap().is_empty());
        assert_eq!(msgs.dialogs, vec!["\"Bad\" is corrupted".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn real_metadata_and_checks() {
        let json = r#"{"Name":"2103305768","DisplayName":"Modded Battery","Version":"1.2.6","PublishedFileID":2103305768,"CreatorUGCIdentity":null,"PathToMetadata":null,"RequiredMods":[],"IsCurrentVersion":false,"IsWorkshopItem":true,"RequiresMods":false}"#;
        let m = ContraptionMetaData::from_json(json).unwrap().unwrap();
        assert!(m.IsWorkshopItem() && !m.IsCurrentVersion() && !m.RequiresMods());
        assert_eq!(save_action_ask_for_save("a.b", &[]), SaveAsk::Invalid("\"a.b\" contains invalid characters".into()));
        assert_eq!(save_action_ask_for_save("my battery", &[]), SaveAsk::Save);
        assert_eq!(save_action_ask_for_save("modded battery", &[ContraptionMetaData::new("Modded Battery")]), SaveAsk::ConfirmOverwrite);
        let e = MigrationEvent { Version: "1.27".into(), VersionCondition: MigrationCondition::OlderThan };
        assert!(e.IsApplicable("1.2.6", true) && !e.IsApplicable("1.28", true) && !e.IsApplicable("1.2.6", false));
        assert_eq!(parse_build_index("1.28").unwrap(), 1 * 10000 + 28 * 1000 + 99);
        assert_eq!(parse_build_index("1.3 beta 2").unwrap(), 13000 + 2);
        // xxHash32 test vectors
        assert_eq!(xxhash(b"", 0), 0x02CC5D05);
        assert_eq!(xxhash(b"a", 0), 0x550D7456);
        assert_eq!(xxhash(b"Nobody inspects the spammish repetition", 0), 0xE2293B2F);
        assert_eq!(dotnet_float(outline_round(1.2345)), "1.23");
        assert_eq!(dotnet_float(outline_round(-0.001)), "-0");
    }
}

/// Contraption.GetPath (obsolete): the `.jaap` in the save directory when it exists, else the one in the sub directory of that name
/// (a null name: "" in `name + ".jaap"`, then GetSubDir(null) throws ArgumentNullException).
pub fn contraption_get_path(save_directory: &Path, name: Option<&str>) -> Result<PathBuf, &'static str> {
    let file = get_file(save_directory, &format!("{}.jaap", name.unwrap_or("")));
    if file.is_file() {
        return Ok(file);
    }
    let name = name.ok_or("ArgumentNullException")?;
    Ok(get_file(&get_sub_dir(save_directory, name), &format!("{name}.jaap")))
}

// Contraption.Contraption: `CContraption { states }` holds the ObjectStates (ceras.rs); the default one has none.
// ContraptionSerialiser.Initialise: the codec is stateless (the HappyTypeBinder functions and the `kind()` formatter table in ceras.rs), so a "new serializer" is the same functions.
// ContraptionSavePoint.Start: ContraptionSavePoint.Main = this is `Game.contraption.save_point`, there from the Game's creation.
