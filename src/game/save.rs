//! Saving and loading: ObjectState (a spawned item's transforms and component data), ObjectStateConverter (the selection to
//! states and back), SerialiseInstructions.GetCurrentState, MonoBehaviourPrototype, Clipboard and the OnBeforeSerialise /
//! OnAfterDeserialise messages. The game stores a contraption with Ceras (`.jaap`), a binary object graph keyed by .NET type
//! names; that codec is not rewritten, so states live in memory (the clipboard and PasteLoadAction) and no file is read or
//! written. Component data is stored for the classes `script_ref` knows (their rewritten structs hold the values); any other
//! class on a copied item is `unimplemented!`, never silently dropped. Public members the structs do not hold (references to
//! other components, runtime-only values) are not stored either;
use super::container::Owner;
use super::physical::SharpAxis;
use super::save_members::{ENUM_TYPES, INHERITED_PROPERTIES, MEMBER_TYPES, PUBLIC_MEMBERS, REFERENCE_MEMBERS};
use super::ceras::Payload;

const NRE: &str = "NullReferenceException: Object reference not set to an instance of an object";
use super::save_members::IMMUTABLE_MEMBERS;
use super::spawn::Instance;
use super::stab::Penetration;
use super::*;
use crate::assets::db::{AssetDb, Component, ObjRef, Prefab};
use crate::assets::mono::Value;
use crate::unity::transform::{self, Trs};

/// Utils.GetHierarchyPath of object `x` in a Transform tree (object 0 the root; `parent` None for the root): its
/// parents' names up to (not with) the root, '/'-joined. The root itself is its own name.
pub(crate) fn hierarchy_path_of<'a>(x: usize, name: &dyn Fn(usize) -> &'a str, parent: &dyn Fn(usize) -> Option<usize>) -> String {
    match parent(x) {
        Some(p) if p != 0 => format!("{}/{}", hierarchy_path_of(p, name, parent), name(x)),
        _ => name(x).to_string(),
    }
}

/// TransformPrototype
#[derive(Default, Clone, Debug, PartialEq)]
pub struct TransformPrototype {
    pub RelativePosition: [f32; 2],
    pub LocalScale: [f32; 3],
    pub RelativeRotation: f32,
}

impl TransformPrototype {
    /// TransformPrototype..ctor
    pub fn new(relative_position: [f32; 2], relative_rotation: f32, local_scale: [f32; 3]) -> Self {
        TransformPrototype { RelativePosition: relative_position, LocalScale: local_scale, RelativeRotation: relative_rotation }
    }
}

/// A GameObject a script Instantiates under one of a spawn's Transforms in Awake or Start (FirearmBehaviour's BulletCasing and
/// MuzzleSmoke, TeslaCoilBehaviour's bolts, EMPBehaviour's and ExplosiveBehaviour's Killzone): a real child, so SerialiseInstructions.Start
/// puts its Transforms in RelevantTransforms, GetCurrentState saves them and CreateNew finds their paths.
#[derive(Clone, Debug)]
pub struct RuntimeChild {
    pub kind: RuntimeKind,
    /// The clone's objects when the rewrite keeps no Instance of it (`Game::runtime_instance`); the root's local Trs under the parent is
    /// `objects[0].trs`, a Muzzle's is `Firearm::muzzle`.
    pub inst: Option<Instance>,
}

#[derive(Clone, Copy, Debug)]
pub enum RuntimeKind {
    /// FirearmBehaviour.muzzleTransform of `Game::firearms[f]`.
    Muzzle(usize),
    /// A clone whose pose only its own entry holds (the BulletCasing, a bolt).
    Plain,
    /// A clone the rewrite keeps as an Instance (by its root Transform id) in `Game::attached_clones`.
    Clone(i32),
}

/// Type.FullName of a script class the asset db names without its namespace (MonoBehaviourPrototype.Type).
fn full_type(class: &str) -> &str {
    match class {
        "LinefyCircularPolyline" => "Linefy.Primitives.LinefyCircularPolyline",
        c => c,
    }
}

/// One stored value of a MonoBehaviourPrototype. `Field` is the struct field's `Field::show` text (floats as exact bits).
#[derive(Clone, Debug, PartialEq)]
pub enum SavedValue {
    Field(String),
    Guid(u128),
    Guids(Vec<u128>),
    Floats(Vec<f32>),
    Vec2s(Vec<[f32; 2]>),
    Bools(Vec<bool>),
    /// SharpAxis[]: (Axis, LowerLimit, UpperLimit, LooseUpperLimit, LooseLowerLimit), and the 4 bytes per item Ceras blits
    /// from past the array (Marshal.SizeOf 24 over a 20-byte stride): a loaded blob's own, zeros for a new one.
    Axes(Vec<([f32; 2], f32, f32, bool, bool)>, Vec<u8>),
    /// BloodContainer.SerialisableDistribution[]: (LiquidID, Amount).
    Distributions(Vec<(String, f32)>),
    /// LiquidContainerLeakingBehaviour.Hole[]: (LocalPos, LocalDir, Intensity) as [px, py, dx, dy, intensity].
    Holes(Vec<[f32; 5]>),
    /// A null value (a member Ceras wrote as null).
    Null,
    /// A value the structs do not model (an old backing field, Bounds, a `Vector4[]`): its .NET type and Ceras payload, kept so a loaded
    /// contraption is written back unchanged.
    Net(String, super::ceras::Payload),
}

impl SavedValue {
    /// A bool[] as the array InjectIntoMonoBehaviour sets. Ceras blits Marshal.SizeOf(bool) = 4 bytes per item from the 1-byte items,
    /// so a loaded payload's first `count` bytes are the items and the rest is memory past the array (kept raw as `Net`).
    pub fn bools(&self) -> Option<Vec<bool>> {
        match self {
            SavedValue::Bools(b) => Some(b.clone()),
            SavedValue::Net(ty, super::ceras::Payload::RawArray(n, b)) if ty == "System.Boolean[]" => b.get(..*n as usize).map(|b| b.iter().map(|&x| x != 0).collect()),
            _ => None,
        }
    }
}

/// MonoBehaviourPrototype.ComponentReferenceLink: the SerialisableIdentity of the referenced object, and the referenced component's type
/// (None for a GameObject reference).
#[derive(Default, Clone, Debug, PartialEq)]
pub struct ComponentReferenceLink {
    pub UniqueIdentity: u128,
    pub ComponentType: Option<String>,
}

impl ComponentReferenceLink {
    /// ComponentReferenceLink..ctor(uniqueIdentity, componentType = null)
    pub fn new(unique_identity: u128, component_type: Option<String>) -> Self {
        ComponentReferenceLink { UniqueIdentity: unique_identity, ComponentType: component_type }
    }
}

/// MonoBehaviourPrototype: the class, whether it was enabled, and its stored fields, properties and reference fields.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct MonoBehaviourPrototype {
    pub Enabled: bool,
    pub Type: String,
    pub storedFields: Vec<(String, SavedValue)>,
    pub storedProperties: Vec<(String, SavedValue)>,
    pub storedReferenceFields: Vec<(String, ComponentReferenceLink)>,
}

/// ObjectState: `Children` and `ChildComponentData` are Dictionaries keyed by hierarchy path, in insertion order.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct ObjectState {
    pub SpawnableAssetName: String,
    pub Root: TransformPrototype,
    pub Children: Vec<(String, TransformPrototype)>,
    pub RootComponentData: Vec<MonoBehaviourPrototype>,
    pub ChildComponentData: Vec<(String, Vec<MonoBehaviourPrototype>)>,
}

impl ObjectState {
    /// ObjectState.constructor
    pub fn new(spawnable_asset_name: String, root: TransformPrototype, children: Vec<(String, TransformPrototype)>) -> Self {
        ObjectState { SpawnableAssetName: spawnable_asset_name, Root: root, Children: children, RootComponentData: Vec::new(), ChildComponentData: Vec::new() }
    }
}

/// Clipboard (the ContraptionOutline it also holds is only drawn by SpawnableOutlineBehaviour and is not kept).
#[derive(Clone, Debug, Default)]
pub struct Clipboard {
    pub States: Vec<ObjectState>,
}

impl Clipboard {
    /// Clipboard.get_Count, Clipboard.get_States and Clipboard.set_States (the field)
    pub fn Count(&self) -> usize {
        self.States.len()
    }

    /// Clipboard.get_Any, Clipboard.constructor (the states are kept; the outline only SpawnableOutlineBehaviour draws is not)
    pub fn Any(&self) -> bool {
        !self.States.is_empty()
    }
}

/// PasteLoadAction.constructor, PasteLoadAction.get_RelevantObjects and PasteLoadAction.set_RelevantObjects: the pasted GameObjects (by their Transform id) and the name its notification shows.
#[derive(Default, Clone, Debug)]
pub struct PasteLoadAction {
    pub displayName: String,
    pub RelevantObjects: Vec<i32>,
}

/// The stab arrays PhysicalBehaviour.OnBeforeSerialise fills, kept on a pasted Physical until OnAfterDeserialise.
#[derive(Clone, Debug, Default)]
pub struct SavedStabs {
    pub victims: Vec<u128>,
    pub durations: Vec<f32>,
    pub axes: Vec<([f32; 2], f32, f32, bool, bool)>,
    pub anchors: Vec<[f32; 2]>,
    pub active: Vec<bool>,
    /// serialisable_slash_victims and serialisable_slash_strength (1.28.45; empty otherwise).
    pub slash_victims: Vec<u128>,
    pub slash_strength: Vec<f32>,
}

/// A rewritten component struct a prototype reads from and injects into.
#[derive(Clone, Copy, Debug)]
enum ScriptRef {
    Physical(usize),
    Limb(usize),
    Person(usize),
    Grip(usize),
    Destroyable(usize),
    TeslaCoil(usize),
    /// CirculationBehaviour, ConnectedNodeBehaviour and SkinMaterialHandler share their limb's index.
    Circulation(usize),
    Node(usize),
    Skin(usize),
    Coupler(usize),
    LiquidLeak(usize),
    JetEngine(usize),
    Chainsaw(usize),
    EnergySword(usize),
    Laser(usize),
    SharpOnAllSides(usize),
    JointHelper(usize),
    AttachmentPoint(usize),
}

/// ObjectStateTypeList.AllowedComponentTypes as its static constructor lists them.
const ALLOWED: &[&str] = &[
    "AcceleratorBoltBehaviour", "AcidPoolBehaviour", "AppearWhenMouseNear", "BlackHoleBehaviour", "DangleBehaviour", "DecalControllerBehaviour", "DetachFromParent",
    "DetachFromParentOnStart", "EditableTintBehaviour", "ExplosiveBehaviour", "FreezeBehaviour", "GeneratorBehaviour", "GlassShardBehaviour", "GuideTemperatureBehaviour",
    "InvisibleOptionInContextMenu", "JointSerialisationHelper", "LaserReceiverBehaviour", "LavaBehaviour", "LayerSerialisationBehaviour", "LightSprite",
    "LineRendererTargetsBehaviour", "OnlyExplosionBehaviour", "ParticleSystemSerialisationHelper", "PointAtBehaviour", "PointToVelocityBehaviour",
    "RotatingSoundBehaviour", "SerialisableIdentity", "SerialiseInstructions", "SerialisedLayerBehaviour", "SyringeExplosionBehaviour", "Undraggable", "WaterBehaviour",
    "WeightlessHelper", "ActOnDestroy",
];

/// The types 1.28.45's ObjectStateTypeList adds (ActOnDestroy moved into the sorted list, which only reorders it).
const ALLOWED_45: &[&str] = &["DebrisComponent", "DulcamarusBehaviour", "InjectableBehaviour"];

// ObjectStateTypeList..cctor: ALLOWED, then every Hover type (allowed_component_type).
/// ObjectStateTypeList..cctor: AllowedComponentTypes is the listed types plus every type of the game's assembly that
/// Hover is assignable from (Hover and its subclasses; mod assemblies are not searched).
/// The Hover types that are abstract: Hover, WireBehaviour, DistanceJointWireBehaviour, FixedJointWireBehaviour, LinkDeviceBehaviour.
const ABSTRACT_HOVERS: &[&str] = &["Hover", "WireBehaviour", "DistanceJointWireBehaviour", "FixedJointWireBehaviour", "LinkDeviceBehaviour"];

/// The allowed Hover types the tools add at run time: the wires, the pin and the phase link (`add_tool_component`).
const TOOL_COMPONENTS: &[&str] = &[
    "AppliedBandageBehaviour", "BloodWireBehaviour", "CableBehaviour", "ChainBehaviour", "CopperWireBehaviour", "EnergyWireBehaviour", "FixedCableBehaviour",
    "HeatPipeBehaviour", "PhaseLinkBehaviour", "PinBehaviour", "RigidWireBehaviour", "RopeBehaviour", "SliderJointWireBehaviour", "SpringCableBehaviour",
    "SteelBindingBehaviour", "UseWireBehaviour", "WoodStrutBehaviour", "WoodenBindingBehaviour",
];

/// A wire, pin or phase link class (`TOOL_COMPONENTS`).
pub(crate) fn is_tool_component(class: &str) -> bool {
    TOOL_COMPONENTS.contains(&class)
}

fn allowed_component_type(class: &str) -> bool {
    const HOVER: &[&str] = &[
        "Hover", "AppliedBandageBehaviour", "BloodWireBehaviour", "CableBehaviour", "ChainBehaviour", "CopperWireBehaviour", "DistanceJointWireBehaviour",
        "EnergyWireBehaviour", "FixedCableBehaviour", "FixedJointWireBehaviour", "HeatPipeBehaviour", "LinkDeviceBehaviour", "NewUseWireBehaviour",
        "PhaseLinkBehaviour", "PinBehaviour", "RigidWireBehaviour", "RopeBehaviour", "SerpentineBeltBehaviour", "SliderJointWireBehaviour",
        "SpringCableBehaviour", "SteelBindingBehaviour", "UseWireBehaviour", "VisualDeletableDetachedToolBehaviour", "WireBehaviour", "WoodStrutBehaviour",
        "WoodenBindingBehaviour",
    ];
    ALLOWED.contains(&class) || HOVER.contains(&class) || (crate::assets::il::is_1_28_45() && ALLOWED_45.contains(&class))
}

impl Game {
    /// A fresh SerialisableIdentity.UniqueIdentity: Guid.NewGuid is not read by the simulation, so the clock's nanoseconds above a counter do.
    fn new_guid(&mut self) -> u128 {
        self.guid_counter += 1;
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        (nanos << 64) | self.guid_counter as u128
    }

    /// SerialisableIdentity.Regenerate on the Transform `tid`.
    pub(crate) fn identity_regenerate(&mut self, tid: i32) {
        let g = self.new_guid();
        self.identities.insert(tid, g);
    }

    /// SerialisableIdentity.UniqueIdentity of the Transform `tid` (Start gave every transform one with a fresh Guid).
    fn identity_of(&mut self, tid: i32) -> u128 {
        if let Some(&g) = self.identities.get(&tid) {
            return g;
        }
        self.identity_regenerate(tid);
        self.identities[&tid]
    }

    fn has_script(prefab: &Prefab, o: usize, class: &str) -> bool {
        prefab.objects[o].components.iter().any(|(_, c)| matches!(c, Component::Script { class: k, .. } if k == class))
    }

    /// BloodContainer.OnBeforeSerialise: SerialisableDistributions from LiquidDistribution, in its enumeration order.
    pub(crate) fn blood_container_on_before_serialise(&mut self, o: Owner) {
        let list = self.bc(o).LiquidDistribution.iter().map(|(k, v)| (super::circulation::liquid_identity(*k).to_string(), v.raw)).collect();
        self.bc_mut(o).SerialisableDistributions = list;
    }

    /// BloodContainer.OnAfterDeserialise: LiquidDistribution rebuilt from SerialisableDistributions (no OnEnterContainer).
    pub(crate) fn blood_container_on_after_deserialise(&mut self, o: Owner) {
        let saved = self.bc(o).SerialisableDistributions.clone();
        let c = self.bc_mut(o);
        c.TotalLiquidAmount = 0.0;
        c.LiquidDistribution = CsDict::default();
        for (id, amount) in saved {
            // Liquid.GetLiquid of an id the registry does not know (a mod's liquid in a contraption) logs an error and gives null;
            // Dictionary.Add(null, ...) then throws, SendMessage logs it, and the entries added before stay.
            let Some(liquid) = super::circulation::liquid_get(&id) else {
                self.log_message_received(&format!("There is no liquid in the registry that matches \"{id}\""), super::menu::LogType::Error);
                super::menu::throw("ArgumentNullException: Value cannot be null.\nParameter name: key");
            };
            let c = self.bc_mut(o);
            c.LiquidDistribution.insert(liquid, super::circulation::RefFloat::new(amount));
            c.TotalLiquidAmount += amount;
        }
    }

    /// AdhesiveCouplerBehaviour.OnBeforeSerialise: the identity of each connection whose Other and Joint are alive (an object without one gives Guid.Empty).
    pub(crate) fn coupler_on_before_serialise(&mut self, c: usize) {
        let mut state = Vec::new();
        for k in self.couplers[c].connections.clone() {
            if self.physicals[k.other].destroyed || self.fixed_joints[k.joint].destroyed {
                continue;
            }
            state.push(if self.object_names.contains_key(&self.root_id(k.other)) {
                let tid = self.physicals[k.other].transform_id;
                self.identity_of(tid)
            } else {
                0
            });
        }
        self.couplers[c].SerialisableState = Some(state);
    }

    /// AdhesiveCouplerBehaviour.OnAfterDeserialise: StickTo the first collider of the new object with each saved identity.
    pub(crate) fn coupler_on_after_deserialise(&mut self, c: usize, objects: &[i32]) {
        for guid in self.couplers[c].SerialisableState.clone().unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object")) {
            let found = self.identities.iter().find(|(t, g)| **g == guid && objects.contains(t)).map(|(t, _)| *t);
            let Some(v) = found.and_then(|t| self.physicals.iter().position(|ph| ph.transform_id == t && !ph.destroyed)) else { continue };
            if let Some(&collider) = self.physicals[v].colliders.first() {
                self.coupler_stick_to(c, v, collider);
            }
        }
        self.couplers[c].stuck = !self.couplers[c].connections.is_empty();
        self.couplers[c].SerialisableState = None;
    }

    /// GripBehaviour.OnBeforeSerialise: the joint's anchors into the saved fields.
    pub(crate) fn grip_on_before_serialise(&mut self, g: usize) {
        let Some(f) = self.grips[g].joint.filter(|&f| !self.fixed_joints[f].destroyed) else { return };
        let s = &self.fixed_joints[f].state;
        let (connected, anchor) = ([s.connected_anchor.x, s.connected_anchor.y], [s.anchor.x, s.anchor.y]);
        let grip = &mut self.grips[g];
        grip.ConnectedAnchor = connected;
        grip.NearestHoldingPos = connected;
        grip.Anchor = anchor;
        grip.GripPosition = [anchor[0], anchor[1], grip.GripPosition[2]];
    }

    /// SkinMaterialHandler.OnAfterDeserialise: a damagePoints array that is not 128 long is replaced by DamagePoint.None entries.
    pub(crate) fn skin_on_after_deserialise(&mut self, l: usize) {
        if self.skins[l].damagePoints.len() != 128 {
            self.skins[l].damagePoints = vec![[0.0, 0.0, 0.0, super::misc::damage_type::NONE as f32]; 128];
        }
    }

    /// The text a struct field shows is what the codec expects for the member's .NET type.
    fn shape_ok(ty: &str, text: &str) -> bool {
        let hex = |t: &str| t.len() == 8 && t.bytes().all(|b| b.is_ascii_hexdigit());
        let parts = |n: usize| text.split(',').count() == n && text.split(',').all(hex);
        match ty {
            "System.Single" => parts(1),
            "System.Boolean" => text == "0" || text == "1",
            "System.Int32" | "System.UInt32" | "System.UInt16" | "System.Byte" | "System.Int16" | "System.Int64" => text.parse::<i64>().is_ok(),
            "UnityEngine.Vector2" => parts(2),
            "UnityEngine.Vector3" => parts(3),
            "UnityEngine.Color" | "UnityEngine.Vector4" | "UnityEngine.Rect" => parts(4),
            "System.Double" => text.len() == 16 && text.bytes().all(|b| b.is_ascii_hexdigit()),
            "System.String" => true,
            "UnityEngine.KeyCode" | "UnityEngine.ForceMode2D" | "UnityEngine.RectTransform+Axis" => text.parse::<i64>().is_ok(),
            "UnityEngine.LayerMask" => text.parse::<i32>().is_ok(),
            "UnityEngine.Vector3Int" => text.split(',').count() == 3 && text.split(',').all(|t| t.parse::<i32>().is_ok()),
            "UnityEngine.Vector3[]" | "UnityEngine.Vector4[]" => text.is_empty() || text.split(',').all(hex),
            _ if ENUM_TYPES.iter().any(|(e, _)| *e == ty) => text.parse::<i64>().is_ok(),
            _ => false,
        }
    }

    /// A member's value as the serialised prefab holds it, in the form `shape_ok` accepts (None for a type the codec has no form for here).
    fn prefab_saved(ty: &str, v: &Value) -> Option<SavedValue> {
        let hex = |f: f32| format!("{:08x}", f.to_bits());
        let floats = |n: usize| -> Option<SavedValue> {
            let Value::Struct(m) = v else { return None };
            if m.len() != n {
                return None;
            }
            m.iter().map(|(_, x)| x.f32().map(hex)).collect::<Option<Vec<_>>>().map(|p| SavedValue::Field(p.join(",")))
        };
        match ty {
            "System.Single" => v.f32().map(|f| SavedValue::Field(hex(f))),
            "System.Boolean" => v.bool().map(|b| SavedValue::Field(if b { "1" } else { "0" }.into())),
            "System.Int32" | "System.UInt32" | "System.UInt16" | "System.Byte" | "System.Int16" | "System.Int64" => v.i64().map(|i| SavedValue::Field(i.to_string())),
            "UnityEngine.Vector2" => floats(2),
            "UnityEngine.Vector3" => floats(3),
            "UnityEngine.Color" | "UnityEngine.Vector4" => floats(4),
            "System.String" => match v {
                Value::Str(s) => Some(SavedValue::Net("System.String".into(), super::ceras::Payload::Str(Some(s.clone())))),
                _ => None,
            },
            "System.Double" => match v {
                Value::Double(d) => Some(SavedValue::Net("System.Double".into(), super::ceras::Payload::F64(d.to_bits()))),
                _ => None,
            },
            // ExplosionParameters (blittable, Ceras writes its memory): uint, Vector3, float, float, bool + 3 pad, EffectSize, float, int.
            "ExplosionCreator+ExplosionParameters" => {
                let Value::Struct(m) = v else { return None };
                let get = |n: &str| m.iter().find(|(k, _)| k == n).map(|(_, x)| x);
                let mut raw = Vec::with_capacity(40);
                raw.extend((get("FragmentationRayCount")?.i64()? as u32).to_le_bytes());
                let Value::Struct(p) = get("Position")? else { return None };
                for (_, x) in p {
                    raw.extend(x.f32()?.to_le_bytes());
                }
                raw.extend(get("Range")?.f32()?.to_le_bytes());
                raw.extend(get("FragmentForce")?.f32()?.to_le_bytes());
                raw.extend([get("CreateParticlesAndSound")?.bool()? as u8, 0, 0, 0]);
                raw.extend((get("EffectSize")?.i64()? as i32).to_le_bytes());
                raw.extend(get("DismemberChance")?.f32()?.to_le_bytes());
                raw.extend((get("BallisticShrapnelCount")?.i64()? as i32).to_le_bytes());
                (raw.len() == 40).then(|| SavedValue::Net(ty.into(), super::ceras::Payload::Raw(raw)))
            }
            // A LayerMask is its serialised m_Bits (a uint), read back as the int the mask converts to.
            "UnityEngine.LayerMask" => match v {
                Value::Struct(m) if m.len() == 1 => m[0].1.i64().map(|i| SavedValue::Field((i as u32 as i32).to_string())),
                _ => None,
            },
            // Matrix4x4's memory is column-major (m00, m10, m20, m30, m01, ...); Unity serialises it as e00, e01, ... (row, column).
            "UnityEngine.Matrix4x4" => {
                let Value::Struct(m) = v else { return None };
                let mut raw = vec![0u8; 64];
                for (name, x) in m {
                    let rc = name.strip_prefix('e').filter(|s| s.len() == 2)?.as_bytes();
                    let (r, c) = ((rc[0] - b'0') as usize, (rc[1] - b'0') as usize);
                    raw[(c * 4 + r) * 4..][..4].copy_from_slice(&x.f32()?.to_le_bytes());
                }
                (m.len() == 16).then(|| SavedValue::Net("UnityEngine.Matrix4x4".into(), super::ceras::Payload::Raw(raw)))
            }
            "System.Single[]" => v.array().and_then(|a| a.iter().map(Value::f32).collect::<Option<Vec<_>>>()).map(SavedValue::Floats),
            "System.Boolean[]" => v.array().and_then(|a| a.iter().map(Value::bool).collect::<Option<Vec<_>>>()).map(SavedValue::Bools),
            "UnityEngine.Vector2[]" => v.array().and_then(|a| {
                a.iter()
                    .map(|e| match e {
                        Value::Struct(m) if m.len() == 2 => Some([m[0].1.f32()?, m[1].1.f32()?]),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()
            }).map(SavedValue::Vec2s),
            _ if ENUM_TYPES.iter().any(|(e, _)| *e == ty) => v.i64().map(|i| SavedValue::Field(i.to_string())),
            _ => None,
        }
    }

    /// MonoBehaviourPrototype.constructor for a script whose class has no `script_ref`: its stored members from the struct the loader
    /// filled (`script_slots`), else what an earlier paste left (`script_loose`), else the serialised prefab (the rewrite does not
    /// change a member its struct does not hold). A member none of them gives is listed in `save_gaps`. None for a destroyed component.
    fn pack_generic(&mut self, class: &str, inst: &Instance, x: usize, k: usize) -> Option<MonoBehaviourPrototype> {
        let id = inst.objects[x].components[k];
        if self.destroyed_components.contains(&id) {
            return None;
        }
        let Component::Script { enabled, fields: prefab_fields, .. } = &inst.prefab.objects[x].components[k].1 else { unreachable!("a script component") };
        let mut proto = MonoBehaviourPrototype { Enabled: self.script_enabled(id, *enabled), Type: full_type(class).into(), storedFields: Vec::new(), storedProperties: Vec::new(), storedReferenceFields: Vec::new() };
        let slot = self.script_slots.get(&id).copied();
        // BloodContainer.OnBeforeSerialise (ObjectStateConverter sends it to the whole spawnable before the states are read).
        let owner = slot.and_then(Self::slot_owner);
        if let Some(o) = owner {
            self.blood_container_on_before_serialise(o);
        }
        // 1.28.45: BuzzerBehaviour.OnBeforeSerialise (Messages.IOnBeforeSerialise, sent the same way).
        if let Some(super::save_slots::Slot::V45(super::v45_new::V::Buzzer(b))) = slot {
            self.buzzer_on_before_serialise(b);
        }
        let extras: Vec<(&str, SavedValue)> = slot.map(|s| self.slot_extras(s, id).into_iter().map(|(n, e)| (n, e.get())).collect()).unwrap_or_default();
        // SerialisedLayerBehaviour's OptionalInts (Active as 4 bytes, Value; an inactive one's Value reads 0).
        let layer = self.layer_editor.serialised_ids.get(&id).filter(|_| class == "SerialisedLayerBehaviour").and_then(|p| self.layer_editor.serialised.get(p)).copied();
        let optional_int = |v: Option<i32>| {
            let mut raw = vec![v.is_some() as u8, 0, 0, 0];
            raw.extend(v.unwrap_or(0).to_le_bytes());
            SavedValue::Net("OptionalInt".into(), Payload::Raw(raw))
        };
        let layer_live: Vec<(&str, SavedValue)> = layer.map(|l| vec![("SortingLayerID", optional_int(l.SortingLayerID)), ("SortingOrder", optional_int(l.SortingOrder))]).unwrap_or_default();
        let members = Self::public_members(class);
        for member in members {
            let (name, is_property) = match member.strip_suffix('*') {
                Some(p) => (p, true),
                None => (*member, false),
            };
            let ty = Self::member_type_of(class, name).expect("a member type for every stored member");
            let live = slot.and_then(|s| self.slot_fields(s).into_iter().find(|(n, _)| *n == name).map(|(_, f)| f.show())).filter(|t| Self::shape_ok(ty, t)).map(SavedValue::Field);
            let live = live
                .or_else(|| extras.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone()).filter(|v| !matches!(v, SavedValue::Field(t) if !Self::shape_ok(ty, t))))
                .or_else(|| layer_live.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone()))
                .or_else(|| owner.filter(|_| name == "SerialisableDistributions").map(|o| SavedValue::Distributions(self.bc(o).SerialisableDistributions.clone())));
            let value = live
                .or_else(|| self.script_loose.get(&id).and_then(|l| l.iter().find(|(n, _)| n == name)).map(|(_, v)| v.clone()))
                .or_else(|| if is_property || !(IMMUTABLE_MEMBERS.contains(&(class, name)) || (crate::assets::il::is_1_28_45() && super::save_members_45::IMMUTABLE_MEMBERS.contains(&(class, name)))) { None } else { prefab_fields.get(name).and_then(|v| Self::prefab_saved(ty, v)) });
            match value {
                Some(v) if is_property => proto.storedProperties.push((name.into(), v)),
                Some(v) => proto.storedFields.push((name.into(), v)),
                None => {
                    self.save_gaps.insert(format!("{class}.{name} ({ty}){}", if is_property { " property" } else { "" }));
                }
            }
        }
        // TeslaCoilLightningBoltBehaviour (no catalog prefab, so not in the member tables): seed, life, layer and Radius.
        if class == "TeslaCoilLightningBoltBehaviour" {
            if let Some(t) = self.tesla_bolts.iter().find(|t| t.id == id) {
                use super::Field;
                for (name, v) in [("seed", t.seed.show()), ("life", t.life.show()), ("layer", (t.layer as i32).to_string()), ("Radius", t.Radius.show())] {
                    proto.storedFields.push((name.into(), SavedValue::Field(v)));
                }
            }
        }
        // DrawableComponent.LateUpdate: an active, enabled polyline caches transform.localToWorldMatrix each frame (Convert runs paused,
        // so the pose is the last LateUpdate's).
        // An edge that never had a LateUpdate while active keeps the prefab's (IMMUTABLE_MEMBERS).
        if let Some(raw) = self.linefy_cached.get(&id).filter(|_| class == "LinefyCircularPolyline") {
            if let Some(f) = proto.storedFields.iter_mut().find(|(n, _)| n == "cachedWorldMatrix") {
                f.1 = SavedValue::Net("UnityEngine.Matrix4x4".into(), Payload::Raw(raw.clone()));
            }
        }
        // LayerSerialisationBehaviour.Layer: a public int the member tables (built from the catalog's prefabs) do not list.
        if class == "LayerSerialisationBehaviour" {
            if let Some(layer) = self.layer_serialisations.values().find(|l| l.id == id).map(|l| l.Layer).or_else(|| self.layer_serialisations_bodyless.get(&id).copied()) {
                proto.storedFields.push(("Layer".into(), SavedValue::Field(layer.to_string())));
            }
        }
        // Component and GameObject fields: a link to the SerialisableIdentity of the object they point at, when it has one.
        let root = inst.objects[0].components[0];
        // HeartMonitorBehaviour.ConnectedLimb: the limb BloodWireBehaviour.SendHeartRate set (a destroyed one is null).
        if class == "HeartMonitorBehaviour" {
            let limb = self.heart_monitors.iter().find(|m| m.id == id).and_then(|m| m.ConnectedLimb).map(|l| self.limbs[l].PhysicalBehaviour);
            if let Some(q) = limb.filter(|&q| !self.physicals[q].destroyed && self.object_names.contains_key(&self.root_id(q))) {
                let t = self.physicals[q].transform_id;
                let link = ComponentReferenceLink { UniqueIdentity: self.identity_of(t), ComponentType: Some("LimbBehaviour".into()) };
                proto.storedReferenceFields.push(("ConnectedLimb".into(), link));
            }
        }
        // 1.28.45: CrossbowBehaviour.AttachedSyringe (a SyringeBehaviour; null once its object is destroyed).
        if class == "CrossbowBehaviour" && crate::assets::il::is_1_28_45() {
            let s = self.v45.crossbows.iter().position(|c| c.id == id).and_then(|c| self.v45.crossbows[c].AttachedSyringe);
            if let Some(q) = s.map(|s| self.syringes[s].Phys).filter(|&q| !self.physicals[q].destroyed && self.object_names.contains_key(&self.root_id(q))) {
                let t = self.physicals[q].transform_id;
                let link = ComponentReferenceLink { UniqueIdentity: self.identity_of(t), ComponentType: Some("SyringeBehaviour".into()) };
                proto.storedReferenceFields.push(("AttachedSyringe".into(), link));
            }
        }
        for (_, field, rty) in REFERENCE_MEMBERS.iter().filter(|(c, ..)| *c == class) {
            // A non-serialised field (LagboxBehaviour's protected MuteBehaviour) reads as null: only Awake sets it.
            let p = match prefab_fields.get(field) {
                Some(Value::PPtr(p)) => *p,
                None => crate::assets::classes::PPtr::default(),
                _ => continue,
            };
            // CarBehaviour.Awake: a null audioSourcePosition becomes its own transform. The fields an Awake sets to GetComponent<T>() (the
            // first T on its own object, null without one): LagboxBehaviour's (TimedGateBehaviour's too) MuteBehaviour, SentryTurretBehaviour's phys.
            let own = p.is_null() && class == "CarBehaviour" && *field == "audioSourcePosition";
            const AWAKE_GET_COMPONENT: &[(&str, &str, &str)] = &[("LagboxBehaviour", "MuteBehaviour", "MuteBehaviour"), ("TimedGateBehaviour", "MuteBehaviour", "MuteBehaviour"), ("SentryTurretBehaviour", "phys", "PhysicalBehaviour")];
            let mute = AWAKE_GET_COMPONENT
                .iter()
                .find(|(c, f, _)| p.is_null() && *c == class && f == field)
                .and_then(|(_, _, t)| inst.prefab.objects[x].components.iter().position(|(_, c)| matches!(c, Component::Script { class, .. } if class == t)));
            if !own && mute.is_none() && (p.is_null() || p.file != 0) {
                continue;
            }
            let found = if own { Some((x, 0)) } else if let Some(c) = mute { Some((x, c)) } else { inst.prefab.index.get(&ObjRef { file: inst.prefab.objects[0].game_object.file, path_id: p.path_id }).copied() };
            let Some((o, c)) = found else { continue };
            // TryGetComponent<SerialisableIdentity>: SerialiseInstructions.Start gives none to an Optout object (the Liquid Canister's Text).
            if rty.is_empty() != (c == usize::MAX) || !self.object_names.contains_key(&root) || Self::has_script(&inst.prefab, o, "Optout") {
                continue;
            }
            let tid = inst.objects[o].components[0];
            let link = ComponentReferenceLink { UniqueIdentity: self.identity_of(tid), ComponentType: if rty.is_empty() { None } else { Some((*rty).into()) } };
            proto.storedReferenceFields.push(((*field).into(), link));
        }
        Some(proto)
    }

    /// MonoBehaviourPrototype.InjectIntoMonoBehaviour for a script whose class has no `script_ref`: a stored member goes into the struct
    /// that holds it, else it is kept in `script_loose` for the next copy; a differing `Enabled` is applied once the clone is kept.
    fn reconstruct_generic(&mut self, inst: &Instance, x: usize, k: usize, proto: &MonoBehaviourPrototype) {
        let class = proto.Type.as_str();
        let id = inst.objects[x].components[k];
        let slot = self.script_slots.get(&id).copied();
        self.inject_slot(slot, id, proto);
        // TeslaCoilLightningBoltBehaviour (a coil's Awake clone, packed beside the member tables): seed, life, layer and Radius.
        if let Some(k) = self.tesla_bolts.iter().position(|t| t.id == id).filter(|_| class == "TeslaCoilLightningBoltBehaviour") {
            use super::Field;
            for (name, value) in &proto.storedFields {
                let SavedValue::Field(text) = value else { continue };
                let t = &mut self.tesla_bolts[k];
                // Debug.LogWarning("Cannot set field ...") on a failure
                let _ = match name.as_str() {
                    "seed" => t.seed.parse(text),
                    "life" => t.life.parse(text),
                    "layer" => str::parse::<i32>(text).map(|v| t.layer = v as u32).map_err(|e| e.to_string()),
                    "Radius" => t.Radius.parse(text),
                    _ => Ok(()),
                };
            }
        }
        let Component::Script { enabled, .. } = &inst.prefab.objects[x].components[k].1 else { unreachable!("a script component") };
        if self.script_enabled(id, *enabled) != proto.Enabled {
            self.pending_enabled.push((class.into(), id, proto.Enabled));
        }
        if !proto.storedReferenceFields.is_empty() {
            self.pending_reference_links.push((class.to_string(), inst.objects[x].components.clone(), proto.storedReferenceFields.clone()));
        }
    }

    /// InjectIntoMonoBehaviour's fields and properties of component `id` (its save slot `slot`): ReflectionCache.GetField / GetProperty with
    /// ShouldSerialiseMember, then SetValue of the converted value.
    fn inject_slot(&mut self, slot: Option<super::save_slots::Slot>, id: i32, proto: &MonoBehaviourPrototype) {
        let class = proto.Type.as_str();
        for (name, value) in proto.storedFields.iter().chain(&proto.storedProperties) {
            if Self::stored_member(class, name).is_none() {
                continue;
            }
            if let (Some(slot), SavedValue::Field(text)) = (slot, value) {
                if let Some((_, field)) = self.slot_fields(slot).into_iter().find(|(n, _)| n == name) {
                    // Debug.LogWarning("Cannot set field ...") on a failure
                    let _ = field.parse(text);
                    continue;
                }
            }
            if let Some(slot) = slot {
                if let Some((_, mut extra)) = self.slot_extras(slot, id).into_iter().find(|(n, _)| n == name) {
                    extra.set(value);
                    continue;
                }
                if let (Some(o), SavedValue::Distributions(d), "SerialisableDistributions") = (Self::slot_owner(slot), value, name.as_str()) {
                    self.bc_mut(o).SerialisableDistributions = d.clone();
                    continue;
                }
            }
            let loose = self.script_loose.entry(id).or_default();
            loose.retain(|(n, _)| n != name);
            loose.push((name.clone(), value.clone()));
        }
    }

    /// ObjectState.IsSerialisableComponent, ObjectState.IsComponentToSerialise (obsolete; calls it) and ReflectionCache.IsComponentToSerialise (its answer cached per type).
    pub(crate) fn is_component_to_serialise(class: &str) -> bool {
        const SKIP_SERIALISATION: &[&str] = &[
            "AddForceBehaviour", "AutomaticSentryController", "CableOtherEndBehaviour", "CosmeticRotationBehaviour", "DangleBehaviour", "GForceMeasureBehaviour",
            "MaintainScaleSignBehaviour", "NixieTubeBehaviour", "SortingLayerChild",
        ];
        const EXCLUDED: &[&str] =
            &["AudioSourceTimeScaleBehaviour", "DeregisterBehaviour", "FrozenInIceBehaviour", "ContextMenuOptionComponent", "DecalControllerBehaviour", "TexturePackApplier", "SerialiseInstructions"];
        // 1.28.45 adds the [SkipSerialisation] DrippingBloodParticleController.
        !SKIP_SERIALISATION.contains(&class) && !EXCLUDED.contains(&class) && !(crate::assets::il::is_1_28_45() && class == "DrippingBloodParticleController")
    }

    /// Utils.GetHierarchyPath (and the obsolete Utils.GetHierachyPath that calls it) of clone object `x` (the root's own name is left out).
    fn hierarchy_path(inst: &Instance, x: usize) -> String {
        hierarchy_path_of(x, &|y| inst.prefab.objects[y].name.as_str(), &|y| inst.objects[y].parent)
    }

    /// The Transform.position, rotation and localScale of clone object `x`. A body or node reads its live pose; a plain Transform
    /// goes through its parents' local poses from the nearest object that has one.
    /// DrawableComponent.LateUpdate of every active, enabled LinefyCircularPolyline: cachedWorldMatrix = transform.localToWorldMatrix.
    /// A clone made before the LateUpdate phase (a spawn in Update, a coroutine's CreateNew) gets it before its Starts run, so
    /// an edge ExistInDetailView.Start switches off (a Fireworks Killzone with the detail view off) still saves its first pose.
    pub(crate) fn linefy_late_update(&mut self) {
        let mut cached = Vec::new();
        for inst in self.unsynced.iter().chain(self.retained.iter()) {
            // A clone whose root body is destroyed (an exploded gas tank) has no LateUpdate: skip it, clone_pose would panic.
            let root = inst.objects[0].components[0];
            if !self.physicals.iter().any(|ph| ph.transform_id == root && !ph.destroyed) && !self.transform_nodes.iter().any(|n| n.id == root) {
                continue;
            }
            for (x, o) in inst.objects.iter().enumerate() {
                if !o.active {
                    continue;
                }
                for (k, (_, c)) in inst.prefab.objects[x].components.iter().enumerate() {
                    let Component::Script { class, enabled, .. } = c else { continue };
                    let id = o.components[k];
                    if class != "LinefyCircularPolyline" || !self.script_enabled(id, *enabled) || self.destroyed_components.contains(&id) {
                        continue;
                    }
                    let mut chain = inst.chain(x);
                    let root = chain.len() - 1;
                    let (t, q, s) = self.clone_pose(inst, 0);
                    chain[root] = Trs { t: [t[0], t[1], t[2], 0.0], q, s: [s[0], s[1], s[2], 0.0] };
                    let m = crate::unity::transform::local_to_world(&chain);
                    cached.push((id, m.iter().flat_map(|f| f.to_le_bytes()).collect()));
                }
            }
        }
        self.linefy_cached.extend(cached);
    }

    /// AddComponent of a script the rewrite keeps no clone component for: it goes on the object of component `on` with its enabled flag.
    pub(crate) fn attach_script(&mut self, on: i32, class: &str, id: i32, enabled: bool) {
        self.add_script_to_clone(on, class, id);
        let added = self.unsynced.iter().chain(self.retained.iter()).any(|i| i.objects.iter().any(|x| x.components.contains(&id)));
        if !enabled && added {
            self.with_component_instance(id, |_, inst, o| {
                let mut prefab = (*inst.prefab).clone();
                if let Some((_, Component::Script { enabled, .. })) = prefab.objects[o].components.last_mut() {
                    *enabled = false;
                }
                inst.prefab = std::sync::Arc::new(prefab);
            });
        }
    }

    pub(super) fn clone_pose(&self, inst: &Instance, x: usize) -> ([f32; 3], [f32; 4], [f32; 3]) {
        let known = |g: &Self, y: usize| -> Option<([f32; 3], [f32; 4], [f32; 3])> {
            let tid = inst.objects[y].components[0];
            if let Some(p) = g.physicals.iter().position(|ph| ph.transform_id == tid && !ph.destroyed) {
                let t = &g.physicals[p].transform;
                return Some((t.position, t.rotation, t.localScale));
            }
            let n = g.transform_nodes.iter().position(|n| n.id == tid)?;
            let (position, rotation) = g.node_world(n);
            let s = g.transform_nodes[n].trs.s;
            Some((position, rotation, [s[0], s[1], s[2]]))
        };
        if let Some(pose) = known(self, x) {
            return pose;
        }
        let mut locals = Vec::new();
        let mut y = x;
        let base = loop {
            if let Some(b) = known(self, y) {
                break b;
            }
            locals.push(inst.objects[y].trs);
            y = inst.objects[y].parent.unwrap_or_else(|| unimplemented!("mod-only: ObjectState: a clone whose root has neither a body nor a node"));
        };
        // Below a body or node that has a parent Transform, the matrix chain goes on through its local pose and its parents
        // (TransformPoint): its world pose with its local scale is wrong under a mirrored or scaled parent (a flipped
        // Bicycle's Dinges/Rod).
        let tid = inst.objects[y].components[0];
        let mut up = vec![Trs { t: [base.0[0], base.0[1], base.0[2], 0.0], q: base.1, s: [base.2[0], base.2[1], base.2[2], 0.0] }];
        if let Some(p) = self.physicals.iter().position(|ph| ph.transform_id == tid && !ph.destroyed) {
            if self.physicals[p].parent.is_some() {
                up = self.local_chain(p);
                up.reverse();
            }
        } else if let Some(n) = self.transform_nodes.iter().position(|n| n.id == tid) {
            if self.transform_nodes[n].parent.is_some() {
                up = std::iter::once(self.node_trs(n)).chain(self.node_parent_chain(n)).collect();
            }
        }
        let own = locals[0];
        let (mut position, mut rotation) = (own.t, own.q);
        for parent in locals[1..].iter().copied().chain(up) {
            position = transform::global_position(&parent, position);
            rotation = transform::global_rotation(&parent, rotation);
        }
        ([position[0], position[1], position[2]], rotation, [own.s[0], own.s[1], own.s[2]])
    }

    /// The rewritten struct holding the `class` component among the component ids of one object.
    fn script_ref(&self, class: &str, comps: &[i32]) -> Option<ScriptRef> {
        let live = |id: i32| comps.contains(&id) && !self.destroyed_components.contains(&id);
        let limb = || self.limbs.iter().position(|l| live(l.id));
        match class {
            "PhysicalBehaviour" => self.physicals.iter().position(|x| live(x.id)).map(ScriptRef::Physical),
            "LimbBehaviour" => limb().map(ScriptRef::Limb),
            "PersonBehaviour" => self.people.iter().position(|x| live(x.id)).map(ScriptRef::Person),
            "GripBehaviour" => self.grips.iter().position(|x| live(x.id)).map(ScriptRef::Grip),
            "DestroyableBehaviour" => self.destroyables.iter().position(|x| live(x.id)).map(ScriptRef::Destroyable),
            "TeslaCoilBehaviour" => self.tesla_coils.iter().position(|x| live(x.id)).map(ScriptRef::TeslaCoil),
            "CirculationBehaviour" => limb().map(ScriptRef::Circulation),
            "ConnectedNodeBehaviour" => limb().map(ScriptRef::Node),
            "SkinMaterialHandler" => limb().map(ScriptRef::Skin),
            "AdhesiveCouplerBehaviour" => self.couplers.iter().position(|x| live(x.id)).map(ScriptRef::Coupler),
            "LiquidContainerLeakingBehaviour" => self.liquid_leaks.iter().position(|x| live(x.id)).map(ScriptRef::LiquidLeak),
            "JetEngineBehaviour" => self.jet_engines.iter().position(|x| live(x.id)).map(ScriptRef::JetEngine),
            "ChainsawBehaviour" => self.chainsaws.iter().position(|x| live(x.id)).map(ScriptRef::Chainsaw),
            "EnergySwordBehaviour" => self.energy_swords.iter().position(|x| live(x.id)).map(ScriptRef::EnergySword),
            "LaserBehaviour" => self.lasers.iter().position(|x| live(x.id)).map(ScriptRef::Laser),
            "SharpOnAllSidesBehaviour" => self.sharp_on_all_sides.iter().position(|x| live(x.id)).map(ScriptRef::SharpOnAllSides),
            "JointSerialisationHelper" | "BatchJointSerialisationHelper" => self.joint_helpers.iter().position(|x| live(x.id)).map(ScriptRef::JointHelper),
            "FirearmAttachmentPointBehaviour" => self.attachment_points.iter().position(|x| live(x.id)).map(ScriptRef::AttachmentPoint),
            _ => None,
        }
    }

    fn script_fields(&mut self, r: ScriptRef) -> Vec<(&'static str, &mut dyn Field)> {
        match r {
            ScriptRef::Physical(i) => self.physicals[i].fields(),
            ScriptRef::Limb(i) => self.limbs[i].fields(),
            ScriptRef::Person(i) => self.people[i].fields(),
            ScriptRef::Grip(i) => self.grips[i].fields(),
            ScriptRef::Destroyable(i) => self.destroyables[i].fields(),
            ScriptRef::TeslaCoil(i) => self.tesla_coils[i].fields(),
            ScriptRef::Circulation(i) => self.circulations[i].fields(),
            ScriptRef::Node(i) => self.nodes[i].fields(),
            ScriptRef::Skin(i) => self.skins[i].fields(),
            ScriptRef::Coupler(i) => self.couplers[i].fields(),
            ScriptRef::LiquidLeak(i) => self.liquid_leaks[i].fields(),
            ScriptRef::JetEngine(i) => self.jet_engines[i].fields(),
            ScriptRef::Chainsaw(i) => self.chainsaws[i].fields(),
            ScriptRef::EnergySword(i) => self.energy_swords[i].fields(),
            ScriptRef::Laser(i) => self.lasers[i].fields(),
            ScriptRef::SharpOnAllSides(i) => self.sharp_on_all_sides[i].fields(),
            // JointHelper's Broken is a bool or a bool[]: stored and injected by hand.
            ScriptRef::JointHelper(_) => Vec::new(),
            ScriptRef::AttachmentPoint(i) => vec![("AttachmentType", &mut self.attachment_points[i].AttachmentType as &mut dyn Field)],
        }
    }

    /// ReflectionCache.GetFields and ReflectionCache.GetProperties (Instance | Public | NonPublic) of a class, with ReflectionCache.ShouldSerialiseMember applied: the
    /// generated table holds, per class, the members Type.GetFields / GetProperties return that MonoBehaviourPrototype stores, in that order (fields, then `*` properties).
    fn public_members(class: &str) -> &'static [&'static str] {
        // ReflectionCache.AddEntry: one CachedTypeInfo per Type, made on first use; the table is that cache, built ahead of time.
        // ReflectionCache..cctor (static setup): the empty Cache, SerialisableComponentTypes, SerialisableTypes and SerialisableMembers dictionaries.
        // CachedTypeInfo..ctor: Type, and the empty per-BindingFlags Fields and Properties dictionaries.
        // CachedTypeInfo.GetFields: the cached MembersByName<FieldInfo>.Flat for the flags (CacheFields on first use).
        // CachedTypeInfo.GetProperties: the cached MembersByName<PropertyInfo>.Flat for the flags (CacheProperties on first use).
        // CachedTypeInfo.CacheFields: new MembersByName<FieldInfo>(Type.GetFields(flags)).
        // CachedTypeInfo.CacheProperties: new MembersByName<PropertyInfo>(Type.GetProperties(flags)).
        // MembersByName`1..ctor: Flat = the array as given (its order is the stored order); ByName maps each Name to its member (`stored_member`).
        // 1.28.45 stores other members for a few classes (save_members_45).
        let v45 = crate::assets::il::is_1_28_45().then(|| super::save_members_45::PUBLIC_MEMBERS.iter().find(|(c, _)| *c == class)).flatten();
        v45.or_else(|| PUBLIC_MEMBERS.iter().find(|(c, _)| *c == class)).map_or(&[][..], |(_, m)| *m)
    }

    /// A stored member's .NET type (MEMBER_TYPES, plus 1.28.45's new members).
    pub(crate) fn member_type_of(class: &str, name: &str) -> Option<&'static str> {
        let v45 = if crate::assets::il::is_1_28_45() { super::save_members_45::MEMBER_TYPES } else { &[] };
        MEMBER_TYPES.iter().chain(v45).find(|(c, n, _)| *c == class && *n == name).map(|(_, _, t)| *t)
    }

    /// MonoBehaviourPrototype.IsSerialisableField and MonoBehaviourPrototype.IsSerialisableProperty (and the obsolete MonoBehaviourPrototype.ShouldSerialiseField and MonoBehaviourPrototype.ShouldSerialiseProperty that call them): Some(is_property) for a stored public member.
    pub(crate) fn stored_member(class: &str, name: &str) -> Option<bool> {
        // ReflectionCache.ShouldSerialiseMember: the answer cached per member; the generated table lists exactly the members it passes (not private, not
        // Obsolete / NonSerialized / SkipSerialisation; a property readable, with a non-private setter and a ReflectionCache.IsSerialisableType type).
        // ReflectionCache.GetField: CachedTypeInfo.GetField by name (null when absent: None here).
        // ReflectionCache.GetProperty: CachedTypeInfo.GetProperty by name.
        // CachedTypeInfo.GetField: the flags' MembersByName<FieldInfo>.ByName lookup (CacheFields on first use).
        // CachedTypeInfo.GetProperty: the flags' MembersByName<PropertyInfo>.ByName lookup (CacheProperties on first use).
        let members = Self::public_members(class);
        members.iter().find_map(|m| match m.strip_suffix('*') {
            Some(p) if p == name => Some(true),
            None if *m == name => Some(false),
            _ => None,
        })
    }

    /// MonoBehaviourPrototype.constructor, MonoBehaviourPrototype.StoreFields and MonoBehaviourPrototype.StoreProperties (with `stored_member`) for one script, with PhysicalBehaviour.OnBeforeSerialise's stab arrays. A Component or GameObject
    /// field that is not skipped (the only one in a covered class is PhysicalBehaviour.ActivationNode, a NodeBehaviour on the object itself) is stored as a reference to the
    /// SerialisableIdentity of its object; the other covered classes' references are all SkipSerialisation, and a ScriptableObject field (Properties) is neither.
    fn pack_component(&mut self, class: &str, comps: &[i32], tid: i32, active: bool) -> MonoBehaviourPrototype {
        let mut proto = MonoBehaviourPrototype { Enabled: true, Type: class.into(), storedFields: Vec::new(), storedProperties: Vec::new(), storedReferenceFields: Vec::new() };
        if class == "SerialisableIdentity" {
            // UniqueIdentity; IdentityForInspector is SkipSerialisation and GameVersion is not a value type.
            proto.storedFields.push(("UniqueIdentity".into(), SavedValue::Guid(self.identity_of(tid))));
            return proto;
        }
        let r = self.script_ref(class, comps).unwrap_or_else(|| unimplemented!("mod-only: ObjectState: component data of {class}"));
        let mut connected_nodes = None;
        // ObjectStateConverter's BroadcastMessage("OnBeforeSerialise") skips an inactive object (a limb its PhysicalBehaviour.Start
        // disintegrated): its fields stay as loaded, the node's SerialisableConnectedNodes the one its waiting Start holds.
        if !active {
            if let ScriptRef::Node(n) = r {
                let held = self.deferred_starts.iter().find_map(|(_, s)| match s {
                    super::misc::PendingStart::Node(l, keep) if *l == n => Some(keep.clone().unwrap_or_default()),
                    _ => None,
                });
                connected_nodes = Some(held.unwrap_or_default());
            }
        } else {
            match r {
                ScriptRef::Grip(g) => self.grip_on_before_serialise(g),
                ScriptRef::Circulation(l) => self.blood_container_on_before_serialise(Owner::Circulation(l)),
                ScriptRef::Coupler(c) => self.coupler_on_before_serialise(c),
                ScriptRef::LiquidLeak(l) => self.liquid_leak_on_before_serialise(l),
                ScriptRef::Node(n) => connected_nodes = Some(self.node_on_before_serialise(n)),
                ScriptRef::AttachmentPoint(pt) => self.attachment_point_on_before_serialise(pt),
                _ => {}
            }
        }
        for (name, field) in self.script_fields(r) {
            match Self::stored_member(class, name) {
                Some(false) => proto.storedFields.push((name.into(), SavedValue::Field(field.show()))),
                Some(true) => proto.storedProperties.push((name.into(), SavedValue::Field(field.show()))),
                None => {}
            }
        }
        if let Some(b) = connected_nodes {
            proto.storedFields.push(("SerialisableConnectedNodes".into(), SavedValue::Bools(b)));
        }
        // JointSerialisationHelper.Broken (bool) and BatchJointSerialisationHelper.Broken (bool[]); Joint and Joints are SkipSerialisation.
        if let ScriptRef::JointHelper(i) = r {
            let h = &self.joint_helpers[i];
            let value = if h.batch { SavedValue::Bools(h.Broken.clone()) } else { SavedValue::Field(h.Broken[0].show()) };
            proto.storedFields.push(("Broken".into(), value));
        }
        // GripBehaviour.CurrentlyHolding: a reference to the held object's SerialisableIdentity (a destroyed one is null, an object without one is not stored).
        if let ScriptRef::Grip(g) = r {
            if let Some(h) = self.grip_held(g) {
                if self.object_names.contains_key(&self.root_id(h)) {
                    let t = self.physicals[h].transform_id;
                    let link = ComponentReferenceLink { UniqueIdentity: self.identity_of(t), ComponentType: Some("PhysicalBehaviour".into()) };
                    proto.storedReferenceFields.push(("CurrentlyHolding".into(), link));
                }
            }
        }
        // FirearmAttachmentPointBehaviour.FirearmAttachment: a reference to the attachment's SerialisableIdentity (an object without one is not stored).
        if let ScriptRef::AttachmentPoint(pt) = r {
            if let Some(att) = self.point_attachment(pt).and_then(|a| self.firearm_attachments[a].physical) {
                if self.object_names.contains_key(&self.root_id(att)) {
                    let t = self.physicals[att].transform_id;
                    let link = ComponentReferenceLink { UniqueIdentity: self.identity_of(t), ComponentType: Some("FirearmAttachmentBehaviour".into()) };
                    proto.storedReferenceFields.push(("FirearmAttachment".into(), link));
                }
            }
        }
        if let ScriptRef::Coupler(c) = r {
            let state = self.couplers[c].SerialisableState.clone().expect("OnBeforeSerialise gave the state");
            proto.storedFields.push(("SerialisableState".into(), SavedValue::Guids(state)));
        }
        if let ScriptRef::LiquidLeak(l) = r {
            let holes = self.liquid_leaks[l].SerialisableHoles.clone().expect("OnBeforeSerialise gave the holes");
            proto.storedFields.push(("SerialisableHoles".into(), SavedValue::Holes(holes)));
        }
        if let ScriptRef::Circulation(l) = r {
            proto.storedFields.push(("SerialisableDistributions".into(), SavedValue::Distributions(self.circulations[l].base.SerialisableDistributions.clone())));
        }
        if let ScriptRef::Physical(p) = r {
            if self.physicals[p].ids.ActivationNode != 0 {
                let link = ComponentReferenceLink::new(self.identity_of(tid), Some("Activations.NodeBehaviour".into()));
                proto.storedReferenceFields.push(("ActivationNode".into(), link));
            }
        }
        // ChainsawBehaviour.OnBeforeSerialise / EnergySwordBehaviour.OnBeforeSerialise: SerialisableVictims is their one stored member.
        match r {
            ScriptRef::Chainsaw(i) => {
                proto.Enabled = self.chainsaws[i].enabled;
                let guids = self.chainsaw_on_before_serialise(i);
                proto.storedFields.push(("SerialisableVictims".into(), SavedValue::Guids(guids)));
            }
            ScriptRef::Laser(i) => proto.Enabled = self.lasers[i].enabled,
            ScriptRef::SharpOnAllSides(i) => {
                proto.Enabled = self.sharp_on_all_sides[i].enabled;
                let guids = self.sharp_on_all_sides_on_before_serialise(i);
                proto.storedFields.push(("SerialisableVictims".into(), SavedValue::Guids(guids)));
            }
            ScriptRef::EnergySword(i) => {
                proto.Enabled = self.energy_swords[i].enabled;
                let guids = self.energy_sword_on_before_serialise(i);
                proto.storedFields.push(("SerialisableVictims".into(), SavedValue::Guids(guids)));
            }
            _ => {}
        }
        if let ScriptRef::Physical(p) = r {
            // The stored members the Physical struct keeps among its links, and the Charge, ShowOutline and BurnIntensity properties.
            let ph = &self.physicals[p];
            let fields: [(&str, String); 12] = [
                ("SendUserPropagation", ph.SendUserPropagation.show()),
                ("ReflectsLasers", ph.ReflectsLasers.show()),
                ("AbsorbsLasers", ph.AbsorbsLasers.show()),
                ("Resizable", ph.Resizable.show()),
                ("ActivationPropagationDelay", ph.ActivationPropagationDelay.show()),
                ("BulletDeflectChanceOverride", ph.BulletDeflectChanceOverride.show()),
                ("HasOutline", ph.HasOutline.show()),
                ("SoundVolumeBoost", ph.SoundVolumeBoost.show()),
                ("MaximumStabVictims", ph.MaximumStabVictims.show()),
                ("LooseEndSize", ph.LooseEndSize.show()),
                ("StabReleaseSpeedModifier", ph.StabReleaseSpeedModifier.show()),
                ("stabWoundCount", ph.stabWoundCount.show()),
            ];
            // 1.28.45's SelectionPriority (a stored member there, kept among the links: link.rs).
            if crate::assets::il::is_1_28_45() && !proto.storedFields.iter().any(|(n, _)| n == "SelectionPriority") {
                proto.storedFields.push(("SelectionPriority".into(), SavedValue::Field(ph.SelectionPriority.show())));
            }
            let charge = if ph.ForceNoCharge { 0.0 } else { ph.charge };
            let props = [("Charge", charge.show()), ("ShowOutline", ph.showOutline.show()), ("BurnIntensity", ph.burnIntensity.show())];
            for (name, v) in fields {
                if !proto.storedFields.iter().any(|(n, _)| n == name) {
                    proto.storedFields.push((name.into(), SavedValue::Field(v)));
                }
            }
            for (name, v) in props {
                if !proto.storedProperties.iter().any(|(n, _)| n == name) {
                    proto.storedProperties.push((name.into(), SavedValue::Field(v)));
                }
            }
            let stabs = self.physical_on_before_serialise(p);
            proto.storedFields.push(("serialisable_victims".into(), SavedValue::Guids(stabs.victims)));
            proto.storedFields.push(("serialisable_durations".into(), SavedValue::Floats(stabs.durations)));
            proto.storedFields.push(("serialisable_axes".into(), { let tail = vec![0; 4 * stabs.axes.len()]; SavedValue::Axes(stabs.axes, tail) }));
            proto.storedFields.push(("serialisable_anchors".into(), SavedValue::Vec2s(stabs.anchors)));
            proto.storedFields.push(("serialisable_active".into(), SavedValue::Bools(stabs.active)));
            if crate::assets::il::is_1_28_45() {
                proto.storedFields.push(("serialisable_slash_victims".into(), SavedValue::Guids(stabs.slash_victims)));
                proto.storedFields.push(("serialisable_slash_strength".into(), SavedValue::Floats(stabs.slash_strength)));
            }
        }
        // LimbBehaviour's StrengthMultipliers (float[]) and Color property; SkinMaterialHandler's RottenProgress and AcidProgress properties.
        let (fields, props): (Vec<(&str, SavedValue)>, Vec<(&str, SavedValue)>) = match r {
            ScriptRef::Limb(l) => (vec![("StrengthMultipliers", SavedValue::Floats(self.limbs[l].StrengthMultipliers.clone()))], vec![("Color", SavedValue::Field(self.limbs[l].Color.show()))]),
            ScriptRef::Skin(l) => (Vec::new(), vec![("RottenProgress", SavedValue::Field(self.skins[l].rottenProgress.show())), ("AcidProgress", SavedValue::Field(self.skins[l].acidProgress.show()))]),
            _ => (Vec::new(), Vec::new()),
        };
        for (name, v) in fields {
            if !proto.storedFields.iter().any(|(n, _)| n == name) {
                proto.storedFields.push((name.into(), v));
            }
        }
        for (name, v) in props {
            if !proto.storedProperties.iter().any(|(n, _)| n == name) {
                proto.storedProperties.push((name.into(), v));
            }
        }
        // StoreFields / StoreProperties walk GetFields / GetProperties: the table's order.
        let members = Self::public_members(class);
        let rank = |name: &str, prop: bool| members.iter().position(|m| m.strip_suffix('*').map_or(!prop && *m == name, |q| prop && q == name)).unwrap_or(usize::MAX);
        proto.storedFields.sort_by_key(|(n, _)| rank(n, false));
        proto.storedProperties.sort_by_key(|(n, _)| rank(n, true));
        proto
    }

    /// The Select of ChainsawBehaviour.OnBeforeSerialise and EnergySwordBehaviour.OnBeforeSerialise: the victims of the connections that are not
    /// marked for deletion and still have their physical, joint and collider, in dictionary order, as their SerialisableIdentity's Guid (a victim
    /// without one throws in GetComponent<SerialisableIdentity>().UniqueIdentity and is caught: Guid.Empty).
    pub(super) fn soft_connection_guids(&mut self, conns: &super::soft_connect::SoftConnections) -> Vec<u128> {
        let mut out = Vec::new();
        for k in conns.keys() {
            let c = *conns.get(k).unwrap();
            if c.should_be_deleted || self.soft_connection_broken(&c) {
                continue;
            }
            // GetComponent<SerialisableIdentity>(): one the victim holds (identities), or the lazy one SerialiseInstructions.Start gives
            // every transform of a spawned object; none (the catch: Guid.Empty) otherwise.
            let tid = self.physicals[c.phys].transform_id;
            out.push(if self.identities.contains_key(&tid) || self.object_names.contains_key(&self.root_id(c.phys)) { self.identity_of(tid) } else { 0 });
        }
        out
    }

    /// The victims' `OnAfterDeserialise` lookup shared by ChainsawBehaviour and EnergySwordBehaviour: the physical of the SerialisableIdentity with
    /// `guid` among the new `objects` (Debug.LogWarning "victim ... does not exist" when there is none), with its first Collider2D.
    pub(super) fn deserialised_victim(&mut self, guid: u128, objects: &[i32]) -> Option<(usize, i32)> {
        let t = self.identities.iter().find(|(t, g)| **g == guid && objects.contains(t)).map(|(t, _)| *t)?;
        let v = self.physicals.iter().position(|ph| ph.transform_id == t && !ph.destroyed)?;
        let id = self.physicals[v].id;
        Some((v, self.collider2d_of(id).unwrap_or(-1)))
    }

    /// PhysicalBehaviour.OnBeforeSerialise: the stab arrays of this stabber's penetrations. A victim without an identity (a map object,
    /// whose GetComponent<SerialisableIdentity>() is null) gives Guid.Empty.
    pub(crate) fn physical_on_before_serialise(&mut self, p: usize) -> SavedStabs {
        let mut s = SavedStabs::default();
        for pen in self.physicals[p].penetrations.clone() {
            let pen = &self.penetration_arena[pen];
            let (victim, duration, axis, anchor, active) = (pen.Victim, pen.Duration, pen.Axis, pen.CurrentConnectedAnchor, pen.Active);
            let guid = match victim {
                Some(v) if self.object_names.contains_key(&self.root_id(v)) => {
                    let tid = self.physicals[v].transform_id;
                    self.identity_of(tid)
                }
                _ => 0,
            };
            s.victims.push(guid);
            s.durations.push(duration);
            s.axes.push((axis.Axis, axis.LowerLimit, axis.UpperLimit, axis.LooseUpperLimit, axis.LooseLowerLimit));
            s.anchors.push(anchor);
            s.active.push(active);
        }
        // 1.28.45: the SlashConnections' victims (Guid.Empty without an identity) and strengths.
        if crate::assets::il::is_1_28_45() {
            for slash in self.physicals[p].SlashConnections.clone() {
                let v = slash.Victim;
                let guid = if self.object_names.contains_key(&self.root_id(v)) { self.identity_of(self.physicals[v].transform_id) } else { 0 };
                s.slash_victims.push(guid);
                s.slash_strength.push(slash.Strength);
            }
        }
        s
    }

    /// ObjectState.PackComponentData and ObjectState.GetPrototypesFor for one object: the scripts to serialise, in component order, then the SerialisableIdentity that
    /// SerialiseInstructions.Start added (unless the prefab has one, which comes in its own place).
    fn pack_object_components(&mut self, inst: &Instance, x: usize, name: Option<String>) -> Vec<MonoBehaviourPrototype> {
        let comps = inst.objects[x].components.clone();
        let tid = comps[0];
        let mut out = Vec::new();
        let scripts: Vec<(usize, String)> = inst.prefab.objects[x]
            .components
            .iter()
            .enumerate()
            .filter_map(|(k, (_, c))| if let Component::Script { class, .. } = c { Some((k, class.clone())) } else { None })
            .collect();
        // The SerialisableIdentity SerialiseInstructions.Start or Reconstruct added (when the prefab has none) comes before the
        // components added at run time after it: those with an instance id at or below `identity_added_at`.
        let identity_at = self.identity_added_at.get(&(tid, false)).or_else(|| self.identity_added_at.get(&(inst.objects[0].components[0], true))).copied();
        let mut identity_slot = None;
        let mut prefab_end = None;
        for (k, class) in &scripts {
            // A destroyed component is no longer on the object for GetComponents.
            if !Self::is_component_to_serialise(class) || self.destroyed_components.contains(&comps[*k]) {
                continue;
            }
            if prefab_end.is_none() && inst.prefab.objects[x].components[*k].0.file == usize::MAX {
                prefab_end = Some(out.len());
            }
            if identity_slot.is_none() && inst.prefab.objects[x].components[*k].0.file == usize::MAX && identity_at.is_some_and(|at| comps[*k] <= at) {
                identity_slot = Some(out.len());
            }
            if inst.prefab.objects[x].components[*k].0.file == usize::MAX && TOOL_COMPONENTS.contains(&class.as_str()) {
                out.push(self.pack_tool_component(class, comps[*k]));
            } else if class == "SerialisableIdentity" || self.script_ref(class, &comps).is_some() {
                {
                // A disintegrated physical's SetActive(false) is its PhysicalBehaviour's `active` (the clone object keeps its flag).
                let active = inst.objects[x].active && !self.physicals.iter().any(|q| !q.active && !q.destroyed && comps.contains(&q.id));
                out.push(self.pack_component(class, &comps, tid, active));
            }
            } else if let Some(p) = self.pack_generic(class, inst, x, *k) {
                out.push(p);
            }
        }
        // The NodeBehaviour PhysicalBehaviour.Awake added (GetOrAddComponent, before any other run-time component): DelaySeconds, Started, Ended
        // (SerialisableState, a SerialisableEvent[], is not a serialisable type). One the prefab carries is among `scripts`.
        let added_node = self.physicals.iter().find(|q| q.transform_id == tid && !q.destroyed).map(|q| q.ids.ActivationNode).filter(|&n| n != 0 && !comps.contains(&n) && !self.destroyed_components.contains(&n));
        if let Some(n) = added_node {
            let node = self.activation_nodes.iter().find(|a| a.id == n).cloned().unwrap_or_default();
            let mut proto = MonoBehaviourPrototype { Enabled: true, Type: "Activations.NodeBehaviour".into(), storedFields: Vec::new(), storedProperties: Vec::new(), storedReferenceFields: Vec::new() };
            proto.storedFields.push(("DelaySeconds".into(), SavedValue::Net("System.Single".into(), Payload::F32(node.DelaySeconds.to_bits()))));
            proto.storedFields.push(("Started".into(), SavedValue::Net("Activations.Channel".into(), Payload::EnumU8(node.Started))));
            proto.storedFields.push(("Ended".into(), SavedValue::Net("Activations.Channel".into(), Payload::EnumU8(node.Ended))));
            let at = prefab_end.unwrap_or(out.len());
            out.insert(at, proto);
            if let Some(s) = identity_slot.as_mut().filter(|s| **s >= at) {
                *s += 1;
            }
        }
        if !scripts.iter().any(|(_, c)| c == "SerialisableIdentity") {
            let identity = self.pack_component("SerialisableIdentity", &comps, tid, true);
            out.insert(identity_slot.unwrap_or(out.len()), identity);
        }
        // The inherited properties (INHERITED_PROPERTIES): useGUILayout is never set (true), hideFlags stays HideFlags.None.
        let name = name.or_else(|| if x == 0 { self.object_names.get(&tid).cloned() } else { None }).unwrap_or_else(|| inst.prefab.objects[x].name.clone());
        let tag = tag_name(inst.prefab.objects[x].tag);
        for proto in &mut out {
            let values = [Payload::Bool(true), Payload::Bool(proto.Enabled), Payload::Str(Some(tag.clone())), Payload::Str(Some(name.clone())), Payload::EnumI32(0)];
            for ((member, ty), v) in INHERITED_PROPERTIES.iter().zip(values) {
                if !proto.storedProperties.iter().any(|(n, _)| n == member) {
                    proto.storedProperties.push(((*member).into(), SavedValue::Net((*ty).into(), v)));
                }
            }
        }
        out
    }

    /// SerialiseInstructions.GetCurrentState (and Utils.GetHierarchyPath via `hierarchy_path`) for the clone `inst` of the spawnable `asset`.
    fn get_current_state(&mut self, inst: &Instance, asset: &str, origin: [f32; 3]) -> ObjectState {
        let (root_position, root_rotation, root_scale) = self.clone_pose(inst, 0);
        let root = Trs { t: [root_position[0], root_position[1], root_position[2], 0.0], q: root_rotation, s: [root_scale[0], root_scale[1], root_scale[2], 0.0] };
        let root_z = super::dump::euler_z(root_rotation);
        // RelevantTransforms (GetComponentsInChildren(true): depth first, an object's runtime children after its prefab ones) without the
        // root and the Optouts.
        let below = inst.descendants(0);
        let (mut clones, mut relevant) = (Vec::new(), Vec::new());
        self.relevant_walk(inst, 0, &below, &mut clones, &mut relevant);
        relevant.retain(|&(k, y)| match k {
            None => !Self::has_script(&inst.prefab, y, "Optout"),
            Some(k) => !Self::has_script(&clones[k].1.prefab, y, "Optout"),
        });
        let path_of = |k: Option<usize>, y: usize| match k {
            None => Self::hierarchy_path(inst, y),
            Some(k) => {
                let (x, c) = &clones[k];
                let own = Self::runtime_path(c, y);
                if *x == 0 { own } else { format!("{}/{own}", Self::hierarchy_path(inst, *x)) }
            }
        };
        let mut children: Vec<(String, TransformPrototype)> = Vec::new();
        for &(k, y) in &relevant {
            let (position, rotation, scale) = match k {
                None => self.clone_pose(inst, y),
                Some(k) => {
                    let (x, c) = &clones[k];
                    let (p, q) = Self::runtime_pose(c, y, &self.transform_chain(inst, *x));
                    let s = c.objects[y].trs.s;
                    (p, q, [s[0], s[1], s[2]])
                }
            };
            let (relative, _) = transform::inverse(&root, [position[0], position[1], position[2], 0.0], rotation);
            let path = path_of(k, y);
            if children.iter().any(|(k, _)| *k == path) {
                panic!("{path} duplicate found");
            }
            children.push((path, TransformPrototype::new([relative[0], relative[1]], super::dump::euler_z(rotation) - root_z, scale)));
        }
        let mut state = ObjectState::new(
            asset.to_string(),
            TransformPrototype::new([root_position[0] - origin[0], root_position[1] - origin[1]], root_z, root_scale),
            children,
        );
        for &(k, y) in &relevant {
            let data = match k {
                None => self.pack_object_components(inst, y, None),
                Some(k) => {
                    let c = &clones[k].1;
                    let name = (y == 0).then(|| c.prefab.objects[0].name.clone());
                    self.pack_object_components(c, y, name)
                }
            };
            state.ChildComponentData.push((path_of(k, y), data));
        }
        state.RootComponentData = self.pack_object_components(inst, 0, None);
        state
    }

    /// GetComponentsInChildren's walk below object `x`: each child's subtree, then the runtime children of `x` (Instantiate makes them
    /// the last siblings). Entries are (None, object) or (Some(index in `clones`), object of that clone).
    fn relevant_walk(&self, inst: &Instance, x: usize, below: &[usize], clones: &mut Vec<(usize, Instance)>, out: &mut Vec<(Option<usize>, usize)>) {
        for &c in below.iter().filter(|&&c| inst.objects[c].parent == Some(x)) {
            out.push((None, c));
            self.relevant_walk(inst, c, below, clones, out);
        }
        for (_, c) in self.runtime_clones(inst.objects[x].components[0]) {
            out.extend((0..c.objects.len()).map(|y| (Some(clones.len()), y)));
            clones.push((x, c));
        }
        // An object SetParent put below this one (reparent.rs: an attached firearm attachment under its gun), after the runtime
        // children; its root keeps its own name (the asset's), not "(Clone)".
        for a in self.attached_clones.get(&inst.objects[x].components[0]).into_iter().flatten().filter(|a| a.driven) {
            let Some(mut i) = self.unsynced.iter().chain(&self.retained).find(|i| i.objects[0].components[0] == a.id).cloned() else { continue };
            i.objects[0].trs = a.local;
            if let Some(name) = self.object_names.get(&a.id) {
                let mut prefab = (*i.prefab).clone();
                prefab.objects[0].name = name.clone();
                i.prefab = std::sync::Arc::new(prefab);
            }
            out.extend((0..i.objects.len()).map(|y| (Some(clones.len()), y)));
            clones.push((x, i));
        }
    }

    /// Utils.GetHierarchyPath of object `y` of a runtime child, from the clone's root (named "<prefab>(Clone)" by `runtime_clones`).
    fn runtime_path(c: &Instance, y: usize) -> String {
        match c.objects[y].parent {
            Some(p) => format!("{}/{}", Self::runtime_path(c, p), c.prefab.objects[y].name),
            None => c.prefab.objects[y].name.clone(),
        }
    }

    /// Transform.position and rotation of object `y` of a runtime child under the Transform chain `owner` (its parent first, the root last).
    fn runtime_pose(c: &Instance, y: usize, owner: &[Trs]) -> ([f32; 3], [f32; 4]) {
        let mut chain = Vec::new();
        let mut at = Some(y);
        while let Some(z) = at {
            chain.push(c.objects[z].trs);
            at = c.objects[z].parent;
        }
        chain.extend_from_slice(owner);
        let (mut p, mut q) = (chain[0].t, chain[0].q);
        for n in &chain[1..] {
            (p, q) = (transform::global_position(n, p), transform::global_rotation(n, q));
        }
        ([p[0], p[1], p[2]], q)
    }

    /// A stand-in Instance of a runtime child the rewrite keeps no Instance of: `prefab`'s objects with the ids its Instantiate took (each
    /// GameObject, then its components) and the root at `root` (its local Trs under the parent; None: the prefab's).
    pub(crate) fn runtime_instance(prefab: std::sync::Arc<Prefab>, ids: &[i32], root: Option<Trs>) -> Instance {
        let mut ids = ids.iter().copied();
        let mut objects: Vec<super::spawn::Clone> = Vec::new();
        for (i, o) in prefab.objects.iter().enumerate() {
            let game_object = ids.next().expect("an id per GameObject");
            let components = o.components.iter().map(|_| ids.next().expect("an id per component")).collect();
            let Component::Transform(t) = &o.components[0].1 else { unreachable!("a GameObject's first component is its Transform") };
            let v3 = |v: [f32; 3]| [v[0], v[1], v[2], 0.0];
            let own = Trs { t: v3(t.local_position), q: t.local_rotation, s: v3(t.local_scale) };
            let trs = if i == 0 { root.unwrap_or(own) } else { own };
            let active = o.active && o.parent.map_or(true, |p| objects[p].active);
            objects.push(super::spawn::Clone { game_object, components, trs, parent: o.parent, active, self_active: o.active, activated: active });
        }
        // Instantiate names the clone's root "<name>(Clone)".
        let mut prefab = (*prefab).clone();
        prefab.objects[0].name = format!("{}(Clone)", prefab.objects[0].name);
        Instance { prefab: std::sync::Arc::new(prefab), objects, ..Default::default() }
    }

    /// Registers a runtime child under the Transform `owner`.
    pub(crate) fn runtime_child_add(&mut self, owner: i32, kind: RuntimeKind, inst: Option<Instance>) {
        self.runtime_children.entry(owner).or_default().push(RuntimeChild { kind, inst });
    }

    /// The runtime children still under the Transform `owner`, each with its root's current local Trs.
    fn runtime_clones(&self, owner: i32) -> Vec<(usize, Instance)> {
        let Some(list) = self.runtime_children.get(&owner) else { return Vec::new() };
        list.iter()
            .enumerate()
            .filter_map(|(j, c)| match c.kind {
                RuntimeKind::Clone(id) => {
                    if self.destroyed_components.contains(&id) {
                        return None;
                    }
                    // SetParent(null) took it out of `attached_clones`: no longer a child.
                    let local = self.attached_clones.get(&owner)?.iter().find(|a| a.id == id)?.local;
                    let mut i = self.unsynced.iter().chain(&self.retained).find(|i| i.objects[0].components[0] == id)?.clone();
                    i.objects[0].trs = local;
                    let mut prefab = (*i.prefab).clone();
                    prefab.objects[0].name = format!("{}(Clone)", prefab.objects[0].name);
                    i.prefab = std::sync::Arc::new(prefab);
                    Some((j, i))
                }
                RuntimeKind::Muzzle(f) => {
                    let mut i = c.inst.clone()?;
                    i.objects[0].trs = self.firearms[f].muzzle?;
                    Some((j, i))
                }
                RuntimeKind::Plain => c.inst.clone().map(|i| (j, i)),
            })
            .collect()
    }

    /// Transform.localPosition, localRotation and localScale of object `y` of the `j`-th runtime child under `owner` (CreateNew).
    fn runtime_set_local(&mut self, owner: i32, j: usize, y: usize, trs: Trs) {
        let kind = self.runtime_children[&owner][j].kind;
        match kind {
            RuntimeKind::Clone(id) => {
                if y == 0 {
                    if let Some(a) = self.attached_clones.get_mut(&owner).and_then(|v| v.iter_mut().find(|a| a.id == id)) {
                        a.local = trs;
                    }
                }
                self.with_component_instance(id, |_, i, _| {
                    i.set_local_position(y, [trs.t[0], trs.t[1], trs.t[2]]);
                    i.set_local_rotation(y, trs.q);
                    i.set_local_scale(y, [trs.s[0], trs.s[1], trs.s[2]]);
                });
            }
            RuntimeKind::Muzzle(f) if y == 0 => self.firearms[f].muzzle = Some(trs),
            _ => {
                if let Some(i) = self.runtime_children.get_mut(&owner).unwrap()[j].inst.as_mut() {
                    i.objects[y].trs = trs;
                }
            }
        }
    }

    /// Transform.Find(`key`) among the runtime children of `inst`'s objects: (owner object, index under it, the clone, its object).
    fn runtime_find(&self, inst: &Instance, key: &str) -> Option<(usize, usize, Instance, usize)> {
        for x in std::iter::once(0).chain(inst.descendants(0)) {
            let prefix = if x == 0 { String::new() } else { format!("{}/", Self::hierarchy_path(inst, x)) };
            let Some(rest) = key.strip_prefix(prefix.as_str()) else { continue };
            for (j, c) in self.runtime_clones(inst.objects[x].components[0]) {
                if let Some(y) = (0..c.objects.len()).find(|&y| Self::runtime_path(&c, y) == rest) {
                    return Some((x, j, c, y));
                }
            }
        }
        None
    }

    /// ObjectState.Reconstruct on object `y` of the `j`-th runtime child under `owner`.
    fn runtime_reconstruct(&mut self, owner: i32, j: usize, y: usize, data: &[MonoBehaviourPrototype], added: &mut usize, deferred: &mut Vec<(i32, MonoBehaviourPrototype, Option<i32>)>) {
        match self.runtime_children[&owner][j].kind {
            RuntimeKind::Clone(id) => self.with_component_instance(id, |g, c, _| g.reconstruct(c, y, data, added, deferred)),
            _ => {
                let mut c = self.runtime_children.get_mut(&owner).unwrap()[j].inst.take().expect("a runtime child's objects");
                self.reconstruct(&mut c, y, data, added, deferred);
                self.runtime_children.get_mut(&owner).unwrap()[j].inst = Some(c);
            }
        }
    }

    /// The Transform ids of the runtime children's objects under the Transform `owner` (live ones, Optouts left out).
    pub(crate) fn runtime_tids(&self, owner: i32) -> Vec<i32> {
        self.runtime_clones(owner).iter().flat_map(|(_, c)| (0..c.objects.len()).filter(|&y| !Self::has_script(&c.prefab, y, "Optout")).map(|y| c.objects[y].components[0]).collect::<Vec<_>>()).collect()
    }

    /// ObjectStateConverter.Convert (the GameObject overload, with ObjectStateConverter.SendMessageToEntireSpawnable's once-per-root): the physical objects' roots to ObjectStates. Identities in each
    /// selected object's hierarchy are regenerated, OnBeforeSerialise runs while the states are read, and a root without a
    /// SerialiseInstructions (not a spawned item) gives no state.
    pub fn object_states_of(&mut self, selected: &[usize], origin: [f32; 3]) -> Vec<ObjectState> {
        // Since 1.28.41 the Regenerate pass goes over each distinct transform.root's hierarchy (GetComponentsInChildren(true)
        // of the root, once), no longer over each selected object's own subtree.
        let mut regenerated: Vec<i32> = Vec::new();
        for &p in selected {
            let tid = self.root_id(p);
            if regenerated.contains(&tid) {
                continue;
            }
            regenerated.push(tid);
            let below = self.with_component_instance(tid, |g, inst, _| {
                let o = inst.objects.iter().position(|x| x.components[0] == tid).unwrap();
                // A runtime child's objects (each got a SerialisableIdentity at SerialiseInstructions.Start) after its parent's.
                std::iter::once(o)
                    .chain(inst.descendants(o))
                    .filter(|&x| x == 0 || !Self::has_script(&inst.prefab, x, "Optout") || Self::has_script(&inst.prefab, x, "SerialisableIdentity"))
                    .map(|x| inst.objects[x].components[0])
                    .chain(std::iter::once(o).chain(inst.descendants(o)).flat_map(|x| g.runtime_tids(inst.objects[x].components[0])))
                    .collect::<Vec<_>>()
            });
            for t in below {
                self.identity_regenerate(t);
            }
        }
        let mut roots: Vec<i32> = Vec::new();
        let mut states = Vec::new();
        for &p in selected {
            let root = self.root_id(p);
            if roots.contains(&root) {
                continue;
            }
            roots.push(root);
            // 1.28.45: root.GetComponentsInChildren<SerialiseInstructions>(true), each one's state (an item parented below the
            // root, as an attached firearm attachment, is saved with it); 1.28.41: only the root's own.
            let items = if crate::assets::il::is_1_28_45() { self.serialise_instructions_in_children(root) } else { vec![root] };
            for item in items {
                let Some(asset) = self.object_names.get(&item).cloned() else { continue };
                let state = self.with_component_instance(item, |g, inst, _| g.get_current_state(inst, &asset, origin));
                states.push(state);
            }
        }
        states
    }

    /// The Transform ids below Transform `tid` (itself first) in hierarchy pre-order, across clones: an object's own children,
    /// then the clones hung below it (attached later, so its last children).
    fn serialise_instructions_in_children(&mut self, tid: i32) -> Vec<i32> {
        let mut out = Vec::new();
        let mut stack = vec![tid];
        while let Some(t) = stack.pop() {
            out.push(t);
            let own: Vec<i32> = self.with_component_instance(t, |_, inst, _| {
                let o = inst.objects.iter().position(|x| x.components[0] == t).unwrap();
                (o + 1..inst.objects.len()).filter(|&d| inst.objects[d].parent == Some(o)).map(|d| inst.objects[d].components[0]).collect()
            });
            let attached = self.attached_clones.get(&t).map(|v| v.iter().map(|c| c.id).collect::<Vec<_>>()).unwrap_or_default();
            stack.extend(own.into_iter().chain(attached).rev());
        }
        out
    }

    /// ObjectState.Reconstruct (with MonoBehaviourPrototype.InjectIntoMonoBehaviour; `Enabled` is applied to the Chainsaw and EnergySword, the other covered structs have none) on clone object `x`: each prototype goes to the first not yet used component of its type; a missing
    /// SerialisableIdentity is added (its instance id allocated), another allowed type cannot be added yet.
    fn reconstruct(&mut self, inst: &mut Instance, x: usize, data: &[MonoBehaviourPrototype], added: &mut usize, deferred: &mut Vec<(i32, MonoBehaviourPrototype, Option<i32>)>) {
        let comps = inst.objects[x].components.clone();
        let tid = comps[0];
        let mut used = vec![false; inst.prefab.objects[x].components.len()];
        for proto in data {
            let class = proto.Type.as_str();
            // ObjectState.TryFindComponent: the first component still in the buffer whose type is the prototype's (IsEquivalentTo); Reconstruct takes it out (`used`).
            let found = inst.prefab.objects[x].components.iter().enumerate().position(|(k, (_, c))| !used[k] && matches!(c, Component::Script { class: n, .. } if full_type(n) == class));
            match found {
                Some(k) => used[k] = true,
                None if class == "SerialisableIdentity" => {
                    // Behind a deferred AddComponent on this object (a Nitroglycerine Flask's ExplosiveBehaviour) it is added after it;
                    // a tool component (a wire, its id taken already) or one GetComponents skips (a DecalControllerBehaviour) is not one.
                    if deferred.iter().any(|(t, q, _)| *t == tid && !TOOL_COMPONENTS.contains(&q.Type.as_str()) && Self::is_component_to_serialise(&q.Type)) {
                        deferred.push((tid, proto.clone(), None));
                    } else {
                        self.allocate_instance_id();
                        self.identity_added_at.insert((tid, false), self.next_instance_id);
                    }
                    // SerialiseInstructions.Start gives one only to the root and to objects without an Optout: this one it no longer has to add.
                    if x == 0 || !inst.prefab.objects[x].components.iter().any(|(_, c)| matches!(c, Component::Script { class: k, .. } if k == "Optout")) {
                        *added += 1;
                    }
                }
                // ActOnDestroy (allowed since 1.28.41; one a GetOrAddComponent of the clone's Awake added, as
                // DestroyWhenAllChildrenDestroyed's, is on the object and matched above): AddComponent<ActOnDestroy> now. Its
                // stored members are none (Event is a class, DestroyWith SkipSerialisation) and it has only OnDestroy, so
                // Enabled changes nothing.
                // Behind a deferred AddComponent on this object (a General Purpose Bomb's PhaseLinkBehaviour) it goes on after it, with its id taken now.
                None if class == "ActOnDestroy" => {
                    if deferred.iter().any(|(t, ..)| *t == tid) {
                        let id = self.allocate_instance_id();
                        deferred.push((tid, proto.clone(), Some(id)));
                    } else {
                        self.act_on_destroy_add(inst, x);
                    }
                    used.push(true);
                    continue;
                }
                // AddComponent of an abstract Hover type returns null (an error is logged) and InjectIntoMonoBehaviour(null) throws, which is caught.
                None if ABSTRACT_HOVERS.contains(&class) => continue,
                // AddComponent<class>: the clone is not kept yet, so `add_allowed_component` runs once it is. A tool component (a wire, pin or
                // link) takes its instance id here.
                None if allowed_component_type(class) => {
                    let id = TOOL_COMPONENTS.contains(&class).then(|| self.allocate_instance_id());
                    deferred.push((tid, proto.clone(), id));
                    continue;
                }
                // Logger.LogWarning("Unexpected component type ...")
                None => continue,
            }
            if class == "SerialisableIdentity" {
                if let Some((_, SavedValue::Guid(g))) = proto.storedFields.iter().find(|(n, _)| n == "UniqueIdentity") {
                    self.identities.insert(tid, *g);
                }
                continue;
            }
            let Some(r) = self.script_ref(class, &comps) else {
                self.reconstruct_generic(&*inst, x, found.expect("a script of the class on the object"), proto);
                continue;
            };
            for (name, value) in proto.storedFields.iter().chain(&proto.storedProperties) {
                if let SavedValue::Field(text) = value {
                    if Self::stored_member(class, name).is_none() {
                        continue;
                    }
                    if let Some((_, field)) = self.script_fields(r).into_iter().find(|(n, _)| n == name) {
                        // Debug.LogWarning("Cannot set field ...") on a failure
                        let _ = field.parse(text);
                    }
                }
            }
            // The Charge and BurnIntensity property setters (charge = value, burnIntensity = value), after the fields as
            // InjectIntoMonoBehaviour sets them: a save made under ForceNoCharge holds field charge != property Charge 0.
            if let ScriptRef::Physical(p) = r {
                for (name, value) in &proto.storedProperties {
                    let SavedValue::Field(t) = value else { continue };
                    let _ = match name.as_str() {
                        "Charge" => super::Field::parse(&mut self.physicals[p].charge, t),
                        "BurnIntensity" => super::Field::parse(&mut self.physicals[p].burnIntensity, t),
                        _ => continue,
                    };
                }
            }
            // The PhysicalBehaviour fields the struct keeps among its links (pack_component's list).
            if let ScriptRef::Physical(p) = r {
                for (name, value) in &proto.storedFields {
                    let SavedValue::Field(t) = value else { continue };
                    let ph = &mut self.physicals[p];
                    let field: &mut dyn super::Field = match name.as_str() {
                        "SendUserPropagation" => &mut ph.SendUserPropagation,
                        "ReflectsLasers" => &mut ph.ReflectsLasers,
                        "AbsorbsLasers" => &mut ph.AbsorbsLasers,
                        "Resizable" => &mut ph.Resizable,
                        "ActivationPropagationDelay" => &mut ph.ActivationPropagationDelay,
                        "BulletDeflectChanceOverride" => &mut ph.BulletDeflectChanceOverride,
                        "HasOutline" => &mut ph.HasOutline,
                        "SoundVolumeBoost" => &mut ph.SoundVolumeBoost,
                        "MaximumStabVictims" => &mut ph.MaximumStabVictims,
                        "LooseEndSize" => &mut ph.LooseEndSize,
                        "StabReleaseSpeedModifier" => &mut ph.StabReleaseSpeedModifier,
                        "stabWoundCount" => &mut ph.stabWoundCount,
                        "SelectionPriority" => &mut ph.SelectionPriority,
                        _ => continue,
                    };
                    let _ = field.parse(t);
                }
            }
            // The members pack_component adds by hand: StrengthMultipliers, set_Color (the sprite colour is rendering), set_RottenProgress, set_AcidProgress.
            for (name, value) in proto.storedFields.iter().chain(&proto.storedProperties) {
                let f32_of = |v: &SavedValue| if let SavedValue::Field(t) = v { u32::from_str_radix(t, 16).ok().map(f32::from_bits) } else { None };
                match (r, name.as_str(), value) {
                    (ScriptRef::Limb(l), "StrengthMultipliers", SavedValue::Floats(f)) => self.limbs[l].StrengthMultipliers = f.clone(),
                    (ScriptRef::Limb(l), "Color", SavedValue::Field(t)) => {
                        let _ = self.limbs[l].Color.parse(t);
                    }
                    (ScriptRef::Skin(l), "RottenProgress", v) => {
                        if let Some(x) = f32_of(v) {
                            self.skins[l].set_rotten(x)
                        }
                    }
                    (ScriptRef::Skin(l), "AcidProgress", v) => {
                        if let Some(x) = f32_of(v) {
                            self.skins[l].set_acid(x)
                        }
                    }
                    _ => {}
                }
            }
            if let ScriptRef::JointHelper(i) = r {
                match proto.storedFields.iter().find(|(n, _)| n == "Broken") {
                    Some((_, SavedValue::Field(t))) if !self.joint_helpers[i].batch => {
                        let _ = self.joint_helpers[i].Broken[0].parse(t);
                    }
                    Some((_, v)) if self.joint_helpers[i].batch && v.bools().is_some_and(|b| b.len() == self.joint_helpers[i].Broken.len()) => self.joint_helpers[i].Broken = v.bools().unwrap(),
                    _ => {}
                }
            }
            if let (ScriptRef::Coupler(c), Some((_, SavedValue::Guids(g)))) = (r, proto.storedFields.iter().find(|(n, _)| n == "SerialisableState")) {
                self.couplers[c].SerialisableState = Some(g.clone());
            }
            if let (ScriptRef::LiquidLeak(l), Some((_, SavedValue::Holes(h)))) = (r, proto.storedFields.iter().find(|(n, _)| n == "SerialisableHoles")) {
                self.liquid_leaks[l].SerialisableHoles = Some(h.clone());
            }
            // InjectIntoMonoBehaviour: the bool[] SerialisableConnectedNodes, which ConnectedNodeBehaviour.Start (still pending on the new node) reads.
            if let (ScriptRef::Node(n), Some(b)) = (r, proto.storedFields.iter().find(|(n, _)| n == "SerialisableConnectedNodes").and_then(|(_, v)| v.bools())) {
                for ps in &mut self.pending_starts {
                    if let super::misc::PendingStart::Node(m, keep) = ps {
                        if *m == n {
                            *keep = Some(b.clone());
                        }
                    }
                }
            }
            if let (ScriptRef::Circulation(l), Some((_, SavedValue::Distributions(d)))) = (r, proto.storedFields.iter().find(|(n, _)| n == "SerialisableDistributions")) {
                self.circulations[l].base.SerialisableDistributions = d.clone();
            }
            if !proto.storedReferenceFields.is_empty() {
                self.pending_reference_links.push((class.to_string(), comps.clone(), proto.storedReferenceFields.clone()));
            }
            // InjectIntoMonoBehaviour: target.enabled = Enabled.
            if let ScriptRef::Laser(i) = r {
                self.lasers[i].enabled = proto.Enabled;
            }
            // MonoBehaviourPrototype.InjectIntoMonoBehaviour: the Guid[] field SerialisableVictims.
            if let (ScriptRef::Chainsaw(_) | ScriptRef::EnergySword(_) | ScriptRef::SharpOnAllSides(_), Some((_, SavedValue::Guids(victims)))) = (r, proto.storedFields.iter().find(|(n, _)| n == "SerialisableVictims")) {
                // InjectIntoMonoBehaviour: target.enabled = Enabled.
                match r {
                    ScriptRef::Chainsaw(i) => {
                        self.chainsaws[i].enabled = proto.Enabled;
                        self.chainsaws[i].SerialisableVictims = victims.clone()
                    }
                    ScriptRef::SharpOnAllSides(i) => {
                        self.sharp_on_all_sides[i].enabled = proto.Enabled;
                        self.sharp_on_all_sides[i].SerialisableVictims = victims.clone()
                    }
                    ScriptRef::EnergySword(i) => {
                        self.energy_swords[i].enabled = proto.Enabled;
                        self.energy_swords[i].SerialisableVictims = victims.clone()
                    }
                    _ => {}
                }
            }
            if let ScriptRef::Physical(p) = r {
                let get = |n: &str| proto.storedFields.iter().find(|(k, _)| k == n).map(|(_, v)| v);
                if let (Some(SavedValue::Guids(victims)), Some(SavedValue::Floats(durations)), Some(SavedValue::Axes(axes, _)), Some(SavedValue::Vec2s(anchors)), Some(active)) =
                    (get("serialisable_victims"), get("serialisable_durations"), get("serialisable_axes"), get("serialisable_anchors"), get("serialisable_active").and_then(SavedValue::bools))
                {
                    // 1.28.45's slash arrays keep their new Guid[0] / float[0] when the save has none.
                    let (slash_victims, slash_strength) = match (get("serialisable_slash_victims"), get("serialisable_slash_strength")) {
                        (Some(SavedValue::Guids(v)), Some(SavedValue::Floats(s))) if crate::assets::il::is_1_28_45() => (v.clone(), s.clone()),
                        _ => (Vec::new(), Vec::new()),
                    };
                    let stabs = SavedStabs { victims: victims.clone(), durations: durations.clone(), axes: axes.clone(), anchors: anchors.clone(), active: active.clone(), slash_victims, slash_strength };
                    self.saved_stabs.insert(p, stabs);
                }
            }
        }
    }

    /// ObjectState.CreateNew: the spawnable instantiated at `origin` + the root's position, its children placed from their relative
    /// poses, the component data injected, then the components a spawn adds. Returns the clone, its links and the added identities.
    fn object_state_create_new(&mut self, db: &mut AssetDb, state: &ObjectState, origin: [f32; 3], flipped: bool) -> Result<(Instance, super::link::Linked, Vec<(i32, MonoBehaviourPrototype, Option<i32>)>), String> {
        let prefab = Self::catalog_prefab(db, &state.SpawnableAssetName).map_err(|_| format!("SpawnableAsset {} not found", state.SpawnableAssetName))?;
        let mut relative = state.Root.RelativePosition;
        relative[0] *= if flipped { -1.0 } else { 1.0 };
        let mut angle = state.Root.RelativeRotation;
        if flipped {
            angle *= -1.0;
        }
        let (mut inst, linked) = self.spawn(db, &prefab, [relative[0] + origin[0], relative[1] + origin[1], origin[2]], super::menu::euler_z_rotation(angle), None)?;
        inst.set_local_scale(0, state.Root.LocalScale);
        let mut added = 0;
        let mut deferred = Vec::new();
        for (key, child) in &state.Children {
            let Some(x) = (1..inst.objects.len()).find(|&x| Self::hierarchy_path(&inst, x) == *key) else {
                // Transform.Find past the prefab's children: a runtime child (Awake made it), the last siblings.
                if let Some((x, j, c, y)) = self.runtime_find(&inst, key) {
                    let (root_position, root_rotation, root_scale) = (inst.position(0), inst.rotation(0), inst.objects[0].trs.s);
                    let root = Trs { t: [root_position[0], root_position[1], root_position[2], 0.0], q: root_rotation, s: root_scale };
                    let world = transform::global_position(&root, [child.RelativePosition[0], child.RelativePosition[1], 0.0, 0.0]);
                    let z = child.RelativeRotation + super::dump::euler_z(inst.rotation(0));
                    let mut parents = Vec::new();
                    let mut at = c.objects[y].parent;
                    while let Some(p) = at {
                        parents.push(c.objects[p].trs);
                        at = c.objects[p].parent;
                    }
                    // The fresh clone's own Transforms are current here (the bodies follow them).
                    let mut at = Some(x);
                    while let Some(p) = at {
                        parents.push(inst.objects[p].trs);
                        at = inst.objects[p].parent;
                    }
                    let ((t, q), _) = transform::set_world(&parents, [world[0], world[1], world[2]], super::menu::euler_z_rotation(z));
                    let s = child.LocalScale;
                    let owner = inst.objects[x].components[0];
                    self.runtime_set_local(owner, j, y, Trs { t: [t[0], t[1], t[2], 0.0], q, s: [s[0], s[1], s[2], 0.0] });
                    if let Some((_, data)) = state.ChildComponentData.iter().find(|(k, _)| k == key) {
                        // `added` counts the prefab objects' identities only (SerialiseInstructions.Start counts the runtime ones itself).
                        self.runtime_reconstruct(owner, j, y, data, &mut 0, &mut deferred);
                    }
                }
                // Debug.LogWarning("Copied transform child not found: ...")
                continue;
            };
            let (root_position, root_rotation, root_scale) = (inst.position(0), inst.rotation(0), inst.objects[0].trs.s);
            let root = Trs { t: [root_position[0], root_position[1], root_position[2], 0.0], q: root_rotation, s: root_scale };
            let world = transform::global_position(&root, [child.RelativePosition[0], child.RelativePosition[1], 0.0, 0.0]);
            inst.set_world_position(x, [world[0], world[1], world[2]]);
            let z = child.RelativeRotation + super::dump::euler_z(inst.rotation(0));
            inst.set_world_rotation(x, super::menu::euler_z_rotation(z));
            inst.set_local_scale(x, child.LocalScale);
            self.refresh_transforms(&inst, &linked);
            if let Some((_, data)) = state.ChildComponentData.iter().find(|(k, _)| k == key) {
                self.reconstruct(&mut inst, x, data, &mut added, &mut deferred);
            }
        }
        self.refresh_transforms(&inst, &linked);
        self.reconstruct(&mut inst, 0, &state.RootComponentData, &mut added, &mut deferred);
        if std::env::var_os("PPG_CDIAG").is_some() {
            // Diagnostics only: the probe's .cdiag line (world rotation x,y,z,w, local z,w, eulerAngles.z bits) per object.
            let h = |f: f32| format!("{:08x}", f.to_bits());
            for x in 0..inst.objects.len() {
                let (q, l) = (inst.rotation(x), inst.objects[x].trs.q);
                eprintln!("cdiag {} q={},{},{},{} lq={},{} ez={}", inst.prefab.objects[x].name, h(q[0]), h(q[1]), h(q[2]), h(q[3]), h(l[2]), h(l[3]), h(super::dump::euler_z(q)));
            }
        }
        // AudioSourceTimeScaleBehaviour and SerialiseInstructions (a catalog spawn also has DeregisterBehaviour and TexturePackApplier;
        // since 1.28.41 CreateNew adds no TexturePackApplier).
        let name = Self::spawnable_name(db, &state.SpawnableAssetName);
        self.spawnable_asset_components(&prefab, &linked, &name, 2);
        if let Some(super::misc::PendingStart::SerialiseInstructions(n, _)) = self.pending_starts.last_mut() {
            *n -= added;
        }
        Ok((inst, linked, deferred))
    }

    /// ObjectState.Reconstruct's `target.AddComponent(component.Type)` and InjectIntoMonoBehaviour for a component of an allowed type that the fresh clone lacks, on the object with
    /// Transform `tid`. The components the game itself adds to a spawned item at runtime are the FreezeBehaviour (the Freeze context menu entry, FreezeSerialisationHelper) and the
    /// LayerSerialisationBehaviour (No Collide); the other allowed types are on their prefabs from the start, so a state this rewrite packed never lacks them.
    fn add_allowed_component(&mut self, tid: i32, proto: &MonoBehaviourPrototype, id: Option<i32>) {
        let components: Vec<i32> = self.unsynced.iter().flat_map(|i| i.objects.iter()).find(|o| o.components[0] == tid).map(|o| o.components.clone()).expect("the kept clone");
        let class = proto.Type.as_str();
        if class == "SerialisableIdentity" {
            self.allocate_instance_id();
            self.identity_added_at.insert((tid, false), self.next_instance_id);
            return;
        }
        let p = self.physicals.iter().position(|q| components.contains(&q.id) && !q.destroyed);
        if let (Some(id), "ActOnDestroy") = (id, class) {
            self.with_component_instance(tid, |g, inst, o| {
                super::destroyable::push_runtime_script(inst, o, "ActOnDestroy", id);
                g.act_on_destroy.entry(tid).or_default();
            });
            return;
        }
        if let Some(id) = id {
            return self.add_tool_component(tid, id, p, proto);
        }
        match class {
            // AddComponent<DebrisComponent> (allowed since 1.28.45; a projectile launched there carries one): no members, no messages.
            "DebrisComponent" => {
                let id = self.allocate_instance_id();
                self.with_component_instance(tid, |_, inst, o| super::destroyable::push_runtime_script(inst, o, "DebrisComponent", id));
            }
            "FreezeBehaviour" => {
                // GetComponent<Rigidbody2D>() in Start needs the object's PhysicalBehaviour; it has no stored member.
                // No Rigidbody2D: Start's rb.velocity throws a NullReferenceException (logged) before anything else; OnDestroy does nothing with a null rb.
                // The component stays on the object (a later save packs it).
                let Some(p) = p else {
                    let cid = self.allocate_instance_id();
                    self.with_component_instance(components[0], |_, inst, o| {
                        let mut prefab = (*inst.prefab).clone();
                        let n = prefab.objects[o].components.len() as i64;
                        prefab.objects[o].components.push((crate::assets::db::ObjRef { file: usize::MAX, path_id: n + 1 }, Component::Script { class: class.into(), execution_order: 0, enabled: proto.Enabled, fields: Value::Struct(Vec::new()) }));
                        inst.prefab = std::sync::Arc::new(prefab);
                        inst.objects[o].components.push(cid);
                    });
                    // A disabled one never Starts.
                    if proto.Enabled {
                        self.pending_starts.push(super::misc::PendingStart::Throw("NullReferenceException: Object reference not set to an instance of an object".into()));
                    }
                    return;
                };
                self.freeze_add_component(p);
                if !proto.Enabled {
                    let id = self.freezes.last().expect("the added FreezeBehaviour").id;
                    self.pending_enabled.push((class.into(), id, false));
                }
            }
            "LayerSerialisationBehaviour" => {
                let cid = self.allocate_instance_id();
                let object_layer = self.with_component_instance(components[0], |_, inst, o| {
                    let mut prefab = (*inst.prefab).clone();
                    let n = prefab.objects[o].components.len() as i64;
                    prefab.objects[o].components.push((crate::assets::db::ObjRef { file: usize::MAX, path_id: n + 1 }, Component::Script { class: class.into(), execution_order: 0, enabled: true, fields: Value::Struct(Vec::new()) }));
                    let layer = prefab.objects[o].layer as i32;
                    inst.prefab = std::sync::Arc::new(prefab);
                    inst.objects[o].components.push(cid);
                    layer
                });
                // The stored public int Layer (default -1 when the prototype has none).
                let layer = proto.storedFields.iter().find(|(n, _)| n == "Layer").and_then(|(_, v)| if let SavedValue::Field(t) = v { str::parse::<i32>(t).ok() } else { None }).unwrap_or(-1);
                // On an object with no body only its GameObject layer (read by nothing here) changes: Start's `Layer == -1` keeps the object's layer.
                let Some(p) = p else {
                    self.layer_serialisations_bodyless.insert(cid, if layer == -1 { object_layer } else { layer });
                    return;
                };
                self.layer_serialisations.insert(p, super::context_menu::LayerSerialisation { id: cid, Layer: layer });
                self.pending_starts.push(super::misc::PendingStart::LayerSerialisation(p));
            }
            // AddComponent<DecalControllerBehaviour>: its DecalDescriptor (a class) and localDecalPositions (a List) are not stored by
            // MonoBehaviourPrototype, so the component has a null descriptor; Start destroys it and logs "Decal controller has an invalid DecalDescriptor!".
            // The physical object's PhysicalBehaviour.Start finds it in GetComponents<DecalControllerBehaviour>() (decalControllers), where it stays.
            "DecalControllerBehaviour" => {
                self.allocate_instance_id();
                if let Some(p) = p {
                    self.physicals[p].decal_controllers.push(super::misc::DecalController { descriptor: super::physical::NULL_DECAL, decals: Vec::new(), holder: None, holder_hidden: false });
                }
            }
            // Nitroglycerine.OnEnterContainer's AddComponent<ExplosiveBehaviour> (a flask or limb that held nitroglycerine when saved).
            "ExplosiveBehaviour" => {
                // Without a PhysicalBehaviour, Awake's GetComponent<PhysicalBehaviour>() is null: Update never checks
                // temperature, burn or charge, and Explode reads only the transform.
                let e = match p {
                    Some(p) => self.add_explosive(p),
                    None => self.add_explosive_on_object(components[0]),
                };
                let id = self.explosives[e].id;
                self.script_slots.insert(id, super::save_slots::Slot::Explosives(e));
                self.inject_slot(Some(super::save_slots::Slot::Explosives(e)), id, proto);
                if !proto.Enabled {
                    self.pending_enabled.push((class.into(), id, false));
                }
            }
            // LayerEditorController's GetOrAddComponent<SerialisedLayerBehaviour>: Awake keeps OriginalSorting, the stored OptionalInts are
            // injected, Start applies them.
            "SerialisedLayerBehaviour" => {
                let Some(p) = p else {
                    // Awake's GetSortingFor(null) throws at phys.SortingGroup (logged); OriginalSorting stays (0, 0) and the
                    // stored OptionalInts are only read by Start, whose SetSortingFor(null, ...) throws the same way.
                    let id = self.allocate_instance_id();
                    self.attach_script(components[0], class, id, proto.Enabled);
                    // The stored OptionalInts are injected all the same (a later save packs them).
                    self.script_loose.insert(id, proto.storedFields.clone());
                    self.log_exception(Some(super::menu::ManagedException(NRE.into())));
                    if proto.Enabled {
                        self.pending_starts.push(super::misc::PendingStart::Throw(NRE.into()));
                    }
                    return;
                };
                // AddComponent: the component is on the object (a later save packs it).
                let id = self.allocate_instance_id();
                self.attach_script(components[0], class, id, proto.Enabled);
                self.layer_editor.serialised_ids.insert(id, p);
                let original = self.serialised_layer_get_sorting_for(p);
                let mut layer = super::layer_editor::SerialisedLayer { OriginalSorting: original, ..Default::default() };
                for (name, value) in &proto.storedFields {
                    // OptionalInt { bool Active; int Value; }: the struct's raw bytes (Active padded to 4).
                    let SavedValue::Net(t, super::ceras::Payload::Raw(b)) = value else { continue };
                    if t != "OptionalInt" || b.len() != 8 {
                        continue;
                    }
                    let v = (b[0] != 0).then(|| i32::from_le_bytes([b[4], b[5], b[6], b[7]]));
                    match name.as_str() {
                        "SortingLayerID" => layer.SortingLayerID = v,
                        "SortingOrder" => layer.SortingOrder = v,
                        _ => {}
                    }
                }
                self.layer_editor.serialised.insert(p, layer);
                // InjectIntoMonoBehaviour's enabled = false holds Start back, and nothing in the game enables it again
                // (SetOrder and the layer editor work on a disabled one all the same).
                if proto.Enabled {
                    self.pending_starts.push(super::misc::PendingStart::SerialisedLayer(p));
                }
            }
            // A state from a version whose prefab had the component (nothing adds one at run time in 1.28.41). Awake: Invisible =
            // !GetComponent<SpriteRenderer>().enabled, phys and the LimbStatusToggled subscription; Start adds the menu button.
            "InvisibleOptionInContextMenu" => {
                let id = self.allocate_instance_id();
                let sprite_enabled = self.with_component_instance(components[0], |_, inst, o| {
                    inst.prefab.objects[o].components.iter().find_map(|(_, x)| match x {
                        Component::SpriteRenderer(r) => Some(r.enabled),
                        _ => None,
                    })
                });
                let Some(sprite_enabled) = sprite_enabled else {
                    // Awake throws at GetComponent<SpriteRenderer>().enabled (logged) before the LimbStatusToggled
                    // subscription; Start adds a button whose Condition (phys.spriteRenderer) never holds, then
                    // UpdateVisibility throws at phys.spriteRenderer (with no phys, ContextMenuOptions throws first).
                    self.add_script_to_clone(components[0], class, id);
                    self.log_exception(Some(super::menu::ManagedException(NRE.into())));
                    if proto.Enabled {
                        self.pending_starts.push(super::misc::PendingStart::Throw(NRE.into()));
                    }
                    return;
                };
                // AddComponent: the component is on the object (a later save packs it).
                self.attach_script(components[0], class, id, proto.Enabled);
                // With no PhysicalBehaviour, Start and each DetailViewToggled throw at phys (menu_start, invisible_update).
                self.invisibles.push(super::misc::Invisible { id, physical: p, Invisible: !sprite_enabled, VisibleInDetailView: true, sprite_enabled });
                let slot = super::save_slots::Slot::Invisibles(self.invisibles.len() - 1);
                self.script_slots.insert(id, slot);
                self.inject_slot(Some(slot), id, proto);
                // InjectIntoMonoBehaviour's enabled = false holds Start (the button and the first UpdateVisibility) back;
                // Awake's LimbStatusToggled subscription stays.
                if proto.Enabled {
                    self.pending_starts.push(super::misc::PendingStart::Menu(super::misc::Menu::Invisible(self.invisibles.len() - 1)));
                }
            }
            // 1.28.45: Chemistry's AddComponent<DulcamarusBehaviour> on a person. Awake: Person = GetComponent<PersonBehaviour>().
            "DulcamarusBehaviour" => {
                let person = self.people.iter().position(|x| components.contains(&x.id)).unwrap_or_else(|| unimplemented!("mod-only: DulcamarusBehaviour on an object without a PersonBehaviour (Awake throws)"));
                let d = self.dulcamarus_add(person);
                let id = self.v45.dulcamarus[d].id;
                self.inject_slot(Some(super::save_slots::Slot::V45(super::v45_new::V::Dulcamarus(d))), id, proto);
                if !proto.Enabled {
                    self.pending_enabled.push((class.into(), id, false));
                }
            }
            // 1.28.45: CrossbowBehaviour.OnLaunch's AddComponent<InjectableBehaviour> on a bolt.
            "InjectableBehaviour" => {
                let id = self.allocate_instance_id();
                self.v45.injectables.push(super::v45_new::Injectable { id, phys: p, LiquidDistributions: None, FxColor: [0.0; 4], destroyed: false, ImbueParticles: None });
                let i = self.v45.injectables.len() - 1;
                self.injectable_attach(i, proto.Enabled);
                self.inject_slot(Some(super::save_slots::Slot::V45(super::v45_new::V::Injectable(i))), id, proto);
                // InjectableBehaviour.Start (enabled only), with the loaded FxColor.
                if proto.Enabled {
                    self.pending_starts.push(super::misc::PendingStart::V45(super::v45_new::V::Injectable(i)));
                }
            }
            _ => unreachable!("{class} is only on a state from a prefab that has it (nothing in the rewrite adds it at runtime)"),
        }
    }

    /// InjectIntoMonoBehaviour of one stored field (ReflectionCache.ShouldSerialiseMember per the member table; a value that does not
    /// parse is the "Cannot set field" warning and leaves the field).
    fn inject_field(proto: &MonoBehaviourPrototype, name: &str, into: &mut dyn super::Field) {
        if Self::stored_member(&proto.Type, name) != Some(false) {
            return;
        }
        if let Some((_, SavedValue::Field(text))) = proto.storedFields.iter().find(|(n, _)| n == name) {
            let _ = into.parse(text);
        }
    }

    /// ObjectState.Reconstruct's AddComponent (its instance id `id` taken then; Awake's GetComponent<PhysicalBehaviour> is `p`) and
    /// InjectIntoMonoBehaviour for a wire, a pin or a phase link: the stored members, then the component waits for LinkReferences
    /// (`tool_component_link`) and its Start, which makes the joint (or the link's ends) from them.
    fn add_tool_component(&mut self, tid: i32, id: i32, p: Option<usize>, proto: &MonoBehaviourPrototype) {
        use super::blood_wire::{BloodWire, SavedWire, WireKind};
        let class = proto.Type.as_str();
        if !proto.Enabled {
            unimplemented!("mod-only: a disabled {class} (InjectIntoMonoBehaviour's enabled = false holds its Start back)");
        }
        self.add_script_to_clone(tid, class, id);
        let inject = |name: &str, into: &mut dyn super::Field| Self::inject_field(proto, name, into);
        let Some(p) = p else {
            // An object without a PhysicalBehaviour (contraption 1872008384's Car root has a FixedCableBehaviour).
            if matches!(class, "PinBehaviour" | "PhaseLinkBehaviour") {
                return self.add_root_tool_component(class, id, tid, proto);
            }
            let mut saved = SavedWire { currentBreakingForce: f32::INFINITY, SpringStrength: 2.0, ..Default::default() };
            inject("currentAnchor", &mut saved.currentAnchor);
            inject("currentConnectedAnchor", &mut saved.currentConnectedAnchor);
            inject("currentDistance", &mut saved.currentDistance);
            inject("currentMaxDistanceOnly", &mut saved.currentMaxDistanceOnly);
            inject("currentBreakingForce", &mut saved.currentBreakingForce);
            inject("targetDistance", &mut saved.targetDistance);
            inject("SpringStrength", &mut saved.SpringStrength);
            inject("WireMaterialName", &mut saved.WireMaterialName);
            let mut wire = super::blood_wire::RootWire::new(WireKind::Cable, id, tid, saved);
            inject("Channel", &mut wire.Channel);
            inject("CanBreak", &mut wire.CanBreak);
            inject("Color", &mut wire.Color);
            wire.kind = wire_kind(class, &wire.saved.WireMaterialName, wire.Channel, wire.CanBreak);
            inject("Invisible", &mut wire.Invisible);
            inject("usedToHaveConnectedBody", &mut wire.usedToHaveConnectedBody);
            inject("WireWidth", &mut wire.WireWidth);
            inject("WireColor", &mut wire.WireColor);
            inject("localRenderEndpoint", &mut wire.localRenderEndpoint);
            inject("breakForceBuffer", &mut wire.breakForceBuffer);
            inject("SoakAmount", &mut wire.SoakAmount);
            inject("SoakColour", &mut wire.SoakColour);
            if !proto.storedReferenceFields.is_empty() {
                self.pending_reference_links.push((class.to_string(), vec![id], proto.storedReferenceFields.clone()));
            }
            self.root_wires.push(wire);
            let r = self.root_wires.len() - 1;
            let b = super::Behaviour::RootWire(r);
            self.behaviour_of.insert(id, (0, b));
            self.behaviours.add(0, b);
            self.pending_starts.push(super::misc::PendingStart::RootWire(r));
            return;
        };
        let mut invisible = false;
        inject("Invisible", &mut invisible);
        if !proto.storedReferenceFields.is_empty() {
            self.pending_reference_links.push((class.to_string(), vec![id], proto.storedReferenceFields.clone()));
        }
        match class {
            "PinBehaviour" => {
                let mut pin = super::pin::Pin {
                    id,
                    physical: p,
                    root: None,
                    Other: None,
                    LocalAnchor: [0.0; 2],
                    BreakingThreshold: f32::INFINITY,
                    UsedToHaveConnectedBody: false,
                    Invisible: invisible,
                    wood: false,
                    Joint: None,
                    initialBreakForce: f32::INFINITY,
                    started: false,
                    renderer: None,
                    constraint: None,
                    IsMouseOverlapping: false,
                    tracker: Default::default(),
                };
                inject("BreakingThreshold", &mut pin.BreakingThreshold);
                inject("LocalAnchor", &mut pin.LocalAnchor);
                inject("UsedToHaveConnectedBody", &mut pin.UsedToHaveConnectedBody);
                // PinSpritePath: the sprite Start loads (rendering); the wood pin's is the one other path the game sets.
                let mut path = String::from("Sprites/Pin");
                inject("PinSpritePath", &mut path);
                pin.wood = path == "Sprites/WoodPin";
                self.pins.push(pin);
                let k = self.pins.len() - 1;
                let b = super::Behaviour::Pin(k);
                self.behaviour_of.insert(id, (0, b));
                self.behaviours.add(0, b);
                self.pending_starts.push(super::misc::PendingStart::Pin(k));
            }
            "PhaseLinkBehaviour" => {
                let mut link = super::link_device::PhaseLink { id, physical: p, Other: None, ..Default::default() };
                inject("LocalOffset", &mut link.LocalOffset);
                inject("OtherLocalOffset", &mut link.OtherLocalOffset);
                link.Invisible = invisible;
                self.phase_links.push(link);
                let l = self.phase_links.len() - 1;
                let b = super::Behaviour::PhaseLink(l);
                self.behaviour_of.insert(id, (0, b));
                self.behaviours.add(0, b);
                self.pending_starts.push(super::misc::PendingStart::PhaseLink(l));
            }
            _ => {
                let mut saved = SavedWire { currentBreakingForce: f32::INFINITY, SpringStrength: 2.0, ..Default::default() };
                inject("currentAnchor", &mut saved.currentAnchor);
                inject("currentConnectedAnchor", &mut saved.currentConnectedAnchor);
                inject("currentDistance", &mut saved.currentDistance);
                inject("currentMaxDistanceOnly", &mut saved.currentMaxDistanceOnly);
                inject("currentBreakingForce", &mut saved.currentBreakingForce);
                inject("targetDistance", &mut saved.targetDistance);
                inject("SpringStrength", &mut saved.SpringStrength);
                inject("WireMaterialName", &mut saved.WireMaterialName);
                // UseWireBehaviour.Color's initialiser (1, 11, 0).
                let (mut channel, mut can_break, mut color) = (0u16, false, [1.0f32, 11.0, 0.0, 1.0]);
                inject("Channel", &mut channel);
                inject("CanBreak", &mut can_break);
                inject("Color", &mut color);
                // The tool kind the class stands for (the spring cables differ only in WireMaterial and SpringStrength).
                let kind = wire_kind(class, &saved.WireMaterialName, channel, can_break);
                // WireColor's initialiser is white.
                let mut wire = BloodWire { kind, id, physical: p, joint: usize::MAX, WireColor: [1.0; 4], Invisible: invisible, Channel: channel, CanBreak: can_break, Color: color, ..BloodWire::new(kind) };
                inject("WireWidth", &mut wire.WireWidth);
                inject("WireColor", &mut wire.WireColor);
                inject("usedToHaveConnectedBody", &mut wire.usedToHaveConnectedBody);
                inject("localRenderEndpoint", &mut wire.localRenderEndpoint);
                inject("SoakAmount", &mut wire.SoakAmount);
                inject("SoakColour", &mut wire.SoakColour);
                wire.start_color = wire.WireColor;
                wire.end_color = wire.WireColor;
                wire.saved = Some(saved);
                self.blood_wires.push(wire);
                let w = self.blood_wires.len() - 1;
                let b = super::Behaviour::BloodWire(w);
                self.behaviour_of.insert(id, (0, b));
                self.behaviours.add(0, b);
                self.pending_starts.push(super::misc::PendingStart::BloodWire(w));
            }
        }
    }

    /// A PinBehaviour or PhaseLinkBehaviour loaded onto an object without a PhysicalBehaviour (Transform `tid`).
    fn add_root_tool_component(&mut self, class: &str, id: i32, tid: i32, proto: &MonoBehaviourPrototype) {
        let inject = |name: &str, into: &mut dyn super::Field| Self::inject_field(proto, name, into);
        let mut invisible = false;
        inject("Invisible", &mut invisible);
        if !proto.storedReferenceFields.is_empty() {
            self.pending_reference_links.push((class.to_string(), vec![id], proto.storedReferenceFields.clone()));
        }
        let b = if class == "PinBehaviour" {
            let mut pin = super::pin::Pin {
                id,
                physical: usize::MAX,
                root: Some(tid),
                Other: None,
                LocalAnchor: [0.0; 2],
                BreakingThreshold: f32::INFINITY,
                UsedToHaveConnectedBody: false,
                Invisible: invisible,
                wood: false,
                Joint: None,
                initialBreakForce: f32::INFINITY,
                started: false,
                renderer: None,
                constraint: None,
                IsMouseOverlapping: false,
                tracker: Default::default(),
            };
            inject("BreakingThreshold", &mut pin.BreakingThreshold);
            inject("LocalAnchor", &mut pin.LocalAnchor);
            inject("UsedToHaveConnectedBody", &mut pin.UsedToHaveConnectedBody);
            let mut path = String::from("Sprites/Pin");
            inject("PinSpritePath", &mut path);
            pin.wood = path == "Sprites/WoodPin";
            self.pins.push(pin);
            self.pending_starts.push(super::misc::PendingStart::Pin(self.pins.len() - 1));
            super::Behaviour::Pin(self.pins.len() - 1)
        } else {
            let mut link = super::link_device::PhaseLink { id, physical: usize::MAX, root: Some(tid), Other: None, Invisible: invisible, ..Default::default() };
            inject("LocalOffset", &mut link.LocalOffset);
            inject("OtherLocalOffset", &mut link.OtherLocalOffset);
            self.phase_links.push(link);
            self.pending_starts.push(super::misc::PendingStart::PhaseLink(self.phase_links.len() - 1));
            super::Behaviour::PhaseLink(self.phase_links.len() - 1)
        };
        self.behaviour_of.insert(id, (0, b));
        self.behaviours.add(0, b);
    }

    /// MonoBehaviourPrototype(original) for a wire, pin or phase link (component `id`) a tool or a load added: Enabled, the stored
    /// members in GetFields order, and each PhysicalBehaviour reference whose object has a SerialisableIdentity.
    fn pack_tool_component(&mut self, class: &str, id: i32) -> MonoBehaviourPrototype {
        let mut proto = MonoBehaviourPrototype { Enabled: true, Type: class.into(), storedFields: Vec::new(), storedProperties: Vec::new(), storedReferenceFields: Vec::new() };
        let mut refs: Vec<(&str, Option<usize>)> = Vec::new();
        let mut text: Vec<(&str, String)> = Vec::new();
        if let Some(w) = self.blood_wires.iter().position(|x| x.id == id) {
            // DistanceJointWireBehaviour.Update keeps currentDistance at typedJoint.distance.
            let f = self.blood_wires[w].joint;
            let live_distance = (f != usize::MAX).then(|| self.fixed_joints[f].distance.map(|d| d.distance)).flatten();
            let x = &self.blood_wires[w];
            let c = &x.current;
            let distance = live_distance.unwrap_or(c.currentDistance);
            for name in Self::public_members(class) {
                let v = match *name {
                    "localRenderEndpoint" => x.localRenderEndpoint.show(),
                    "WireWidth" => x.WireWidth.show(),
                    "WireColor" => x.WireColor.show(),
                    "WireMaterialName" => c.WireMaterialName.show(),
                    "usedToHaveConnectedBody" => x.usedToHaveConnectedBody.show(),
                    "currentConnectedAnchor" => c.currentConnectedAnchor.show(),
                    "currentAnchor" => c.currentAnchor.show(),
                    "currentBreakingForce" => c.currentBreakingForce.show(),
                    "currentDistance" => distance.show(),
                    "currentMaxDistanceOnly" => c.currentMaxDistanceOnly.show(),
                    "targetDistance" => c.targetDistance.show(),
                    "SpringStrength" => c.SpringStrength.show(),
                    "Invisible" => x.Invisible.show(),
                    "breakForceBuffer" => x.breakForceBuffer.show(),
                    "Color" => x.Color.show(),
                    "Channel" => x.Channel.show(),
                    "CanBreak" => x.CanBreak.show(),
                    "SoakAmount" => x.SoakAmount.show(),
                    "SoakColour" => x.SoakColour.show(),
                    n => unreachable!("{class}.{n} is not a stored wire member"),
                };
                text.push((name, v));
            }
            refs = vec![("physicalBehaviour", Some(x.physical)), ("otherPhysicalBehaviour", x.other)];
        } else if let Some(r) = self.root_wires.iter().position(|x| x.id == id) {
            let x = &self.root_wires[r];
            // Only DistanceJointWireBehaviour.Tick reads a current* member back (currentDistance): Start never reached the rest.
            let c = &x.saved;
            for name in Self::public_members(class) {
                let v = match *name {
                    "localRenderEndpoint" => x.localRenderEndpoint.show(),
                    "WireWidth" => x.WireWidth.show(),
                    "WireColor" => x.WireColor.show(),
                    "WireMaterialName" => c.WireMaterialName.show(),
                    "usedToHaveConnectedBody" => x.usedToHaveConnectedBody.show(),
                    "currentConnectedAnchor" => c.currentConnectedAnchor.show(),
                    "currentAnchor" => c.currentAnchor.show(),
                    "currentBreakingForce" => c.currentBreakingForce.show(),
                    "Invisible" => x.Invisible.show(),
                    "breakForceBuffer" => x.breakForceBuffer.show(),
                    "currentDistance" => c.currentDistance.show(),
                    "currentMaxDistanceOnly" => c.currentMaxDistanceOnly.show(),
                    "targetDistance" => c.targetDistance.show(),
                    "SpringStrength" => c.SpringStrength.show(),
                    "Color" => x.Color.show(),
                    "Channel" => x.Channel.show(),
                    "CanBreak" => x.CanBreak.show(),
                    "SoakAmount" => x.SoakAmount.show(),
                    "SoakColour" => x.SoakColour.show(),
                    n => unreachable!("{class}.{n} is not a stored wire member"),
                };
                text.push((name, v));
            }
            // physicalBehaviour is null.
            refs = vec![("physicalBehaviour", None), ("otherPhysicalBehaviour", x.other)];
        } else if let Some(k) = self.pins.iter().position(|x| x.id == id) {
            let x = &self.pins[k];
            let path = if x.wood { "Sprites/WoodPin" } else { "Sprites/Pin" };
            for name in Self::public_members(class) {
                let v = match *name {
                    "BreakingThreshold" => x.BreakingThreshold.show(),
                    "LocalAnchor" => x.LocalAnchor.show(),
                    "UsedToHaveConnectedBody" => x.UsedToHaveConnectedBody.show(),
                    "PinSpritePath" => path.to_string(),
                    "Invisible" => x.Invisible.show(),
                    n => unreachable!("{class}.{n} is not a stored pin member"),
                };
                text.push((name, v));
            }
            refs = vec![("Other", x.Other)];
        } else if let Some(l) = self.phase_links.iter().position(|x| x.id == id) {
            let x = &self.phase_links[l];
            for name in Self::public_members(class) {
                let v = match *name {
                    "LocalOffset" => x.LocalOffset.show(),
                    "OtherLocalOffset" => x.OtherLocalOffset.show(),
                    "Invisible" => x.Invisible.show(),
                    n => unreachable!("{class}.{n} is not a stored link member"),
                };
                text.push((name, v));
            }
            refs = vec![("Other", x.Other)];
        }
        proto.storedFields = text.into_iter().map(|(n, v)| (n.to_string(), SavedValue::Field(v))).collect();
        for (c, name, ty) in REFERENCE_MEMBERS {
            if *c != class {
                continue;
            }
            let Some(&(_, Some(q))) = refs.iter().find(|(n, _)| n == name) else { continue };
            // `(bool)component && component.TryGetComponent<SerialisableIdentity>`
            if self.physicals[q].destroyed || !self.object_names.contains_key(&self.root_id(q)) {
                continue;
            }
            let t = self.physicals[q].transform_id;
            let link = ComponentReferenceLink { UniqueIdentity: self.identity_of(t), ComponentType: Some((*ty).into()) };
            proto.storedReferenceFields.push((name.to_string(), link));
        }
        proto
    }

    /// LinkReferencesToMonoBehaviour for a loaded wire, pin or phase link (component `id`): `field` is set to physical `q`.
    fn tool_component_link(&mut self, id: i32, field: &str, q: usize) {
        if let Some(r) = self.root_wires.iter().position(|x| x.id == id) {
            match field {
                // Start connects the joint to it.
                "otherPhysicalBehaviour" => self.root_wires[r].other = Some(q),
                // Awake's GetComponent<PhysicalBehaviour>() of an object without one is null whatever was saved.
                _ => {}
            }
            return;
        }
        if let Some(w) = self.blood_wires.iter().position(|x| x.id == id) {
            match field {
                "otherPhysicalBehaviour" => self.blood_wires[w].other = Some(q),
                // Awake's GetComponent<PhysicalBehaviour>(): a save only ever links the wire's own one.
                "physicalBehaviour" if q == self.blood_wires[w].physical => {}
                "physicalBehaviour" => unimplemented!("mod-only: a wire whose physicalBehaviour is another object's"),
                _ => {}
            }
        } else if let Some(k) = self.pins.iter().position(|x| x.id == id) {
            self.pins[k].Other = Some(q);
        } else if let Some(l) = self.phase_links.iter().position(|x| x.id == id) {
            self.phase_links[l].Other = Some(q);
        }
    }

    /// ObjectStateConverter.Convert (the ObjectState overload): the states back into the game, then OnAfterDeserialise.
    /// Returns the new roots' Transform ids; a state that cannot be created shows the two notifications and is skipped.
    pub fn object_states_convert(&mut self, states: &[ObjectState], origin: [f32; 3], flipped: bool) -> Vec<i32> {
        self.object_states_create(states, origin, flipped, true).into_iter().flatten().collect()
    }

    /// ObjectState.CreateNew for each state (each one's root, or its exception message); `convert` adds what ObjectStateConverter.Convert does around it.
    pub(crate) fn object_states_create(&mut self, states: &[ObjectState], origin: [f32; 3], flipped: bool, convert: bool) -> Vec<Result<i32, String>> {
        let (roots, objects, components) = self.object_states_make(states, origin, flipped, convert);
        if convert {
            self.link_references(&objects);
            self.send_after_deserialise(&objects, &components);
        } else {
            // CreateNew alone links nothing.
            self.pending_reference_links.clear();
        }
        roots
    }

    /// ObjectState.CreateNew for each state, then each one's ObjectState.LinkReferences with Convert's pool (`full`: every new object's
    /// identity) or an empty one; no scale flip and no OnAfterDeserialise.
    pub(crate) fn object_states_create_linked(&mut self, states: &[ObjectState], origin: [f32; 3], full: bool) -> Vec<Result<i32, String>> {
        let (roots, objects, _) = self.object_states_make(states, origin, false, false);
        self.link_references(if full { &objects } else { &[] });
        roots
    }

    /// The CreateNew loop of object_states_create: the roots (or errors), every new object's Transform id and every new component id.
    fn object_states_make(&mut self, states: &[ObjectState], origin: [f32; 3], flipped: bool, convert: bool) -> (Vec<Result<i32, String>>, Vec<i32>, Vec<i32>) {
        let mut db = self.assets.take().expect("ObjectStateConverter.Convert without the game's data");
        let mut roots = Vec::new();
        let mut components: Vec<i32> = Vec::new();
        let mut objects: Vec<i32> = Vec::new();
        for state in states {
            match self.object_state_create_new(&mut db, state, origin, flipped) {
                Ok((mut inst, linked, deferred)) => {
                    if convert {
                        let s = inst.objects[0].trs.s;
                        inst.set_local_scale(0, [s[0] * if flipped { -1.0 } else { 1.0 }, s[1], s[2]]);
                        self.refresh_transforms(&inst, &linked);
                    }
                    roots.push(Ok(inst.objects[0].components[0]));
                    for o in &inst.objects {
                        components.extend(o.components.iter().copied());
                        objects.push(o.components[0]);
                    }
                    self.unsynced.push(inst);
                    for (tid, proto, id) in &deferred {
                        self.add_allowed_component(*tid, proto, *id);
                    }
                    for (class, id, enabled) in std::mem::take(&mut self.pending_enabled) {
                        // InjectIntoMonoBehaviour: target.enabled = Enabled.
                        if enabled {
                            self.behaviour_enable(&class, id);
                        } else {
                            self.behaviour_disable(&class, id);
                        }
                    }
                }
                Err(e) => {
                    if convert {
                        self.notify(crate::il!(str "ObjectStateConverter::Convert" 1).into());
                        self.notify(crate::il!(str "ObjectStateConverter::Convert" 2).into());
                    }
                    roots.push(Err(e));
                }
            }
        }
        self.assets = Some(db);
        (roots, objects, components)
    }

    /// ObjectState.LinkReferences, ObjectState.LinkReferencesFrom and MonoBehaviourPrototype.LinkReferencesToMonoBehaviour for the prototypes Reconstruct injected: each reference field is looked up
    /// by Guid in the SerialisableIdentity pool of the new `objects` (Debug.LogWarning "No object with identity ... found" when it is absent) and set to that object's component of the stored type.
    /// ActivationNode's is its own object's NodeBehaviour, which Awake's GetOrAddComponent already put there; another object's would need its component.
    pub(crate) fn link_references(&mut self, objects: &[i32]) {
        for (class, comps, refs) in std::mem::take(&mut self.pending_reference_links) {
            let own = comps[0];
            for (name, link) in refs {
                // Type.GetField(name) with ShouldSerialiseField: PhysicalBehaviour.ActivationNode and FirearmAttachmentPointBehaviour.FirearmAttachment are the stored reference fields.
                let Some(target) = self.identities.iter().find(|(t, g)| **g == link.UniqueIdentity && objects.contains(t)).map(|(t, _)| *t) else { continue };
                if TOOL_COMPONENTS.contains(&class.as_str()) {
                    // A stored PhysicalBehaviour field (REFERENCE_MEMBERS) gets the target's PhysicalBehaviour (TryGetComponent; Debug.LogWarning when it
                    // has none). A link of another component type would not convert to the field (SetValue throws, caught).
                    if !REFERENCE_MEMBERS.iter().any(|(c, n, _)| *c == class && *n == name) || link.ComponentType.as_deref() != Some("PhysicalBehaviour") {
                        continue;
                    }
                    let Some(q) = self.physicals.iter().position(|ph| ph.transform_id == target && !ph.destroyed) else { continue };
                    self.tool_component_link(own, &name, q);
                    continue;
                }
                if (class.as_str(), name.as_str()) == ("FirearmAttachmentPointBehaviour", "FirearmAttachment") {
                    let a = self.firearm_attachments.iter().position(|x| x.physical.is_some_and(|p| self.physicals[p].transform_id == target && !self.physicals[p].destroyed));
                    if let Some(pt) = self.attachment_points.iter().position(|p| comps.contains(&p.id)) {
                        self.attachment_points[pt].FirearmAttachment = a;
                    }
                    continue;
                }
                // GripBehaviour.CurrentlyHolding: the target's PhysicalBehaviour (GripBehaviour.Start then attaches it when isHolding).
                if (class.as_str(), name.as_str()) == ("GripBehaviour", "CurrentlyHolding") {
                    let q = self.physicals.iter().position(|ph| ph.transform_id == target && !ph.destroyed);
                    if let (Some(q), Some(g)) = (q, self.grips.iter().position(|x| comps.contains(&x.id))) {
                        self.grips[g].CurrentlyHolding = Some(q);
                    }
                    continue;
                }
                // 1.28.45: CrossbowBehaviour.AttachedSyringe: the target's SyringeBehaviour (GetComponent finds a subclass too).
                if (class.as_str(), name.as_str()) == ("CrossbowBehaviour", "AttachedSyringe") {
                    let s = self.syringes.iter().position(|s| self.physicals[s.Phys].transform_id == target && !self.physicals[s.Phys].destroyed);
                    if let (Some(s), Some(c)) = (s, self.v45.crossbows.iter().position(|c| comps.contains(&c.id))) {
                        self.v45.crossbows[c].AttachedSyringe = Some(s);
                    }
                    continue;
                }
                // HeartMonitorBehaviour.ConnectedLimb: the target's LimbBehaviour.
                if (class.as_str(), name.as_str()) == ("HeartMonitorBehaviour", "ConnectedLimb") {
                    let l = self.limbs.iter().position(|l| self.physicals[l.PhysicalBehaviour].transform_id == target && !self.physicals[l.PhysicalBehaviour].destroyed);
                    if let (Some(l), Some(h)) = (l, self.heart_monitors.iter().position(|m| comps.contains(&m.id))) {
                        self.heart_monitors[h].ConnectedLimb = Some(l);
                    }
                    continue;
                }
                if (class.as_str(), name.as_str()) != ("PhysicalBehaviour", "ActivationNode") {
                    continue;
                }
                if target != own {
                    // serialisableIdentity.GetComponent("NodeBehaviour"): the target object's node (its PhysicalBehaviour's ActivationNode, else a NodeBehaviour of its own);
                    // a target without one leaves the field as it was.
                    let components: Vec<i32> = self.unsynced.iter().chain(&self.retained).flat_map(|i| i.objects.iter()).find(|o| o.components[0] == target).map(|o| o.components.clone()).unwrap_or_default();
                    let node = self.physicals.iter().find(|q| q.transform_id == target && !q.destroyed).map(|q| q.ids.ActivationNode).filter(|&n| n != 0).or_else(|| self.activation_nodes.iter().find(|n| components.contains(&n.id)).map(|n| n.id));
                    if let (Some(node), Some(p)) = (node, self.physicals.iter().position(|q| comps.contains(&q.id) && !q.destroyed)) {
                        self.physicals[p].ids.ActivationNode = node;
                    }
                }
            }
        }
    }

    /// ObjectStateConverter.SendMessageToEntireSpawnable(list, "OnAfterDeserialise", list): the receivers on the new objects. `objects` are the new
    /// Transform ids (the SerialisableIdentity pool) and `components` every component id they hold.
    pub(crate) fn send_after_deserialise(&mut self, objects: &[i32], components: &[i32]) {
        // The physicals' OnAfterDeserialise (physical_on_after_deserialise).
        for p in 0..self.physicals.len() {
            if self.destroyed_components.contains(&self.physicals[p].id) || !components.contains(&self.physicals[p].id) {
                continue;
            }
            self.physical_on_after_deserialise(p, objects);
        }
        for c in 0..self.couplers.len() {
            if components.contains(&self.couplers[c].id) && !self.destroyed_components.contains(&self.couplers[c].id) {
                self.coupler_on_after_deserialise(c, objects);
            }
        }
        for k in 0..self.cannot_shares.len() {
            let (id, substitute, name) = (self.cannot_shares[k].id, self.cannot_shares[k].substitute, self.cannot_shares[k].name.clone());
            if components.contains(&id) && !self.destroyed_components.contains(&id) {
                self.cannot_share_on_after_deserialise(id, substitute, &name);
            }
        }
        for j in 0..self.jet_engines.len() {
            if components.contains(&self.jet_engines[j].id) && !self.destroyed_components.contains(&self.jet_engines[j].id) {
                self.jet_engine_on_after_deserialise(j);
            }
        }
        // BloodContainer.OnAfterDeserialise of the other containers (the flasks, syringes, valves ...), in component order.
        let mut containers: Vec<(i32, Owner)> = self.script_slots.iter().filter(|(id, _)| components.contains(id) && !self.destroyed_components.contains(id)).filter_map(|(id, s)| Some((*id, Self::slot_owner(*s)?))).collect();
        containers.sort_by_key(|(id, _)| *id);
        for (_, o) in containers {
            self.unity_callback(|g| g.blood_container_on_after_deserialise(o));
        }
        // BloodContainer.OnAfterDeserialise (a limb's CirculationBehaviour) and SkinMaterialHandler.OnAfterDeserialise
        for l in 0..self.limbs.len() {
            if components.contains(&self.limbs[l].id) && !self.destroyed_components.contains(&self.limbs[l].id) {
                self.unity_callback(|g| g.blood_container_on_after_deserialise(Owner::Circulation(l)));
                self.skin_on_after_deserialise(l);
            }
        }
        for l in 0..self.lasers.len() {
            if components.contains(&self.lasers[l].id) && !self.destroyed_components.contains(&self.lasers[l].id) {
                self.laser_on_after_deserialise(l);
            }
        }
        for s in 0..self.sharp_on_all_sides.len() {
            if components.contains(&self.sharp_on_all_sides[s].id) && !self.destroyed_components.contains(&self.sharp_on_all_sides[s].id) {
                self.sharp_on_all_sides_on_after_deserialise(s, objects);
            }
        }
        // ChainsawBehaviour.OnAfterDeserialise and EnergySwordBehaviour.OnAfterDeserialise
        for c in 0..self.chainsaws.len() {
            if components.contains(&self.chainsaws[c].id) && !self.destroyed_components.contains(&self.chainsaws[c].id) {
                self.chainsaw_on_after_deserialise(c, objects);
            }
        }
        for e in 0..self.energy_swords.len() {
            if components.contains(&self.energy_swords[e].id) && !self.destroyed_components.contains(&self.energy_swords[e].id) {
                self.energy_sword_on_after_deserialise(e, objects);
            }
        }
        // JointSerialisationHelper.OnAfterDeserialise and BatchJointSerialisationHelper.OnAfterDeserialise
        for h in 0..self.joint_helpers.len() {
            if components.contains(&self.joint_helpers[h].id) && !self.destroyed_components.contains(&self.joint_helpers[h].id) {
                self.joint_helper_on_after_deserialise(h);
            }
        }
        // The tesla coils' (tesla_coil_on_after_deserialise).
        for t in 0..self.tesla_coils.len() {
            if components.contains(&self.tesla_coils[t].id) {
                self.tesla_coil_on_after_deserialise(t);
            }
        }
        // 1.28.45: CrossbowBehaviour.OnAfterDeserialise
        for c in 0..self.v45.crossbows.len() {
            if components.contains(&self.v45.crossbows[c].id) && !self.destroyed_components.contains(&self.v45.crossbows[c].id) {
                self.crossbow_on_after_deserialise(c);
            }
        }
    }

    /// PhysicalBehaviour.OnAfterDeserialise: the saved stabs, their victims found among `objects` (the new Transform ids)
    /// through the pool's identities.
    pub(crate) fn physical_on_after_deserialise(&mut self, p: usize, objects: &[i32]) {
        let Some(stabs) = self.saved_stabs.remove(&p) else { return };
        for i in 0..stabs.victims.len() {
            let victim = self.identities.iter().find(|(t, g)| **g == stabs.victims[i] && objects.contains(t)).map(|(t, _)| *t);
            let Some(v) = victim.and_then(|t| self.physicals.iter().position(|ph| ph.transform_id == t && !ph.destroyed)) else {
                // Debug.LogWarning("Stab victim with ID ... does not exist")
                continue;
            };
            let (lower, upper) = (stabs.axes[i].1, stabs.axes[i].2);
            let axis = SharpAxis::new(stabs.axes[i].0, lower, upper, stabs.axes[i].3, stabs.axes[i].4);
            let pen = self.penetration_arena.len();
            self.penetration_arena.push(Penetration { Stabber: p, Axis: axis, Active: stabs.active[i], CurrentConnectedAnchor: stabs.anchors[i], Duration: stabs.durations[i], ..Default::default() });
            self.do_stab(p, v, axis, pen);
        }
        // 1.28.45: each SlashCollider's OverlapCollider (Bounds, CollidingDebris, Objects; a Collider2D[8] buffer), and
        // a hit whose PhysicalBehaviour's SerialisableIdentity is a saved victim slashed again at rigidbody.position with
        // the saved strength.
        let Some(colliders) = self.physicals[p].SlashColliders.clone().filter(|c| !c.is_empty() && !stabs.slash_victims.is_empty()) else { return };
        for c in colliders {
            if c == 0 {
                // A null SlashColliders entry: val.OverlapCollider throws, which ends OnAfterDeserialise.
                return;
            }
            let fixtures = self.world.collider_fixtures(c);
            let mut hits = self.overlap_collider(&fixtures, super::machinegun::LAYERS_TO_HIT);
            hits.truncate(8);
            for (victim, other) in hits {
                let Some(v) = victim else { continue };
                let Some(guid) = self.identities.get(&self.physicals[v].transform_id).copied() else { continue };
                let Some(k) = stabs.slash_victims.iter().position(|g| *g == guid) else { continue };
                self.sync_pending();
                let at = self.world.body(self.physicals[p].rigidbody.body).xf.p;
                let s = self.do_slash(p, [at.x, at.y], v, c, other);
                self.physicals[p].SlashConnections[s].Strength = stabs.slash_strength[k];
            }
        }
    }

    /// TeslaCoilBehaviour.OnAfterDeserialise: unfit.
    pub(crate) fn tesla_coil_on_after_deserialise(&mut self, t: usize) {
        self.tesla_coils[t].unfit = true;
    }

    /// ClipboardControllerBehaviour.Awake (Main = this: `Game::clipboard`) and `ObjectStateInClipboard = new Clipboard(selection, center)`.
    pub(crate) fn clipboard_store(&mut self, selected: &[usize], center: [f32; 3]) {
        let live: Vec<usize> = selected.iter().copied().filter(|&p| !self.physicals[p].destroyed).collect();
        let states = self.object_states_of(&live, center);
        self.clipboard = Some(Clipboard { States: states });
    }

    /// DragTool.Update's copy: the clipboard filled, then the "Copied N objects" notification.
    pub(super) fn clipboard_copy(&mut self, selected: &[usize], center: [f32; 3]) {
        self.clipboard_store(selected, center);
        self.notify(format!("Copied {} objects", selected.len()));
    }

    /// DragTool.Update's paste: "Pasted N objects", then the PasteLoadAction of the converted states registered with the undo history.
    pub(super) fn clipboard_paste(&mut self, at: [f32; 3]) {
        let Some(states) = self.clipboard.as_ref().filter(|c| c.Any()).map(|c| c.States.clone()) else { return };
        self.notify(format!("Pasted {} objects", self.clipboard.as_ref().unwrap().Count()));
        self.paste_states(&states, at, false, "Paste");
    }

    /// `UndoControllerBehaviour.RegisterAction(new PasteLoadAction(ObjectStateConverter.Convert(states, origin, flipped), name))`.
    pub fn paste_states(&mut self, states: &[ObjectState], at: [f32; 3], flipped: bool, name: &str) {
        let roots = self.object_states_convert(states, at, flipped);
        self.undo_register_action(PasteLoadAction { displayName: name.into(), RelevantObjects: roots });
    }

    /// ContextMenuBehaviour.CopyAction: the selection (the caller checks isOpen and HasObject) copied around `center` (the menu's world position), no
    /// notification. Whether the menu hid.
    pub(crate) fn context_copy_action(&mut self, selected: &[usize], center: [f32; 3]) -> bool {
        self.clipboard_store(selected, center);
        true
    }

    /// ContextMenuBehaviour.PasteAction: with something copied (HasCopied), its states pasted at the menu's world position. Whether the menu hid.
    pub(crate) fn context_paste_action(&mut self, at: [f32; 3]) -> bool {
        let Some(states) = self.clipboard.as_ref().map(|c| c.States.clone()) else { return false };
        self.paste_states(&states, at, false, "Paste");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play::{new_game, step};

    fn ctl(g: &mut Game, key: &str, down: bool, up: bool) {
        g.input.controls.insert(key.into(), super::super::tool::Control { down, held: false, up });
    }

    fn frames(g: &mut Game, n: usize) {
        for _ in 0..n {
            step(g, 1.0 / 60.0).unwrap();
            for c in g.input.controls.values_mut() {
                c.down = false;
                c.up = false;
            }
        }
    }

    /// A Brick spawned and at rest, copied with the copy key, pasted with the paste key, and the paste converted again: the same
    /// ObjectState bar the identities. Undo removes the paste.
    #[test]
    fn copy_paste_round_trips_a_brick() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut g = new_game(&game, "Default", Some("Brick"), None, 0).unwrap();
        frames(&mut g, 30);
        let first = g.physicals.len();
        let c = g.BoundingBox.center;
        let Some(hit) = g.raycast_first(crate::b2::math::Vec2::new(c[0], c[1]), [0.0, -1.0], 1.0e4) else { return };
        g.input.mouse = [c[0], hit.point.y + 0.6, 0.0];
        ctl(&mut g, "spawnRight", false, true);
        frames(&mut g, 1);
        assert!(g.physicals.len() > first);
        frames(&mut g, 300);
        // An ActOnDestroy added at run time (saved since 1.28.41) comes after the identity SerialiseInstructions.Start added.
        let root = g.root_id(first);
        g.act_on_destroy_get_or_add(root);
        g.select(first, false);
        ctl(&mut g, crate::il!(str "DragTool::Update" 0), true, false);
        frames(&mut g, 1);
        let clip = g.clipboard.clone().expect("copy fills the clipboard");
        assert_eq!((clip.Count(), clip.States[0].SpawnableAssetName.as_str()), (1, "Brick"));
        let types = |s: &ObjectState| s.RootComponentData.iter().map(|m| m.Type.clone()).collect::<Vec<_>>();
        let t = types(&clip.States[0]);
        assert_eq!(t[t.len() - 2..], ["SerialisableIdentity", "ActOnDestroy"]);
        assert!(g.context_condition("HasCopied"));
        let physical = clip.States[0].RootComponentData.iter().find(|m| m.Type == "PhysicalBehaviour").expect("PhysicalBehaviour data");
        assert!(physical.storedFields.iter().any(|(n, _)| n == "Temperature") && physical.storedFields.iter().any(|(n, _)| n == "serialisable_victims"));
        // Paste somewhere else.
        let count = g.physicals.len();
        g.input.mouse = [c[0] + 4.0, hit.point.y + 0.6, 0.0];
        ctl(&mut g, crate::il!(str "DragTool::Update" 2), false, true);
        frames(&mut g, 1);
        assert!(g.physicals.len() > count, "paste spawned nothing");
        assert!(matches!(g.undo.history.last(), Some(super::super::undo::UndoAction::Paste(a)) if a.RelevantObjects.len() == 1));
        let pasted = (count..g.physicals.len()).find(|&p| g.object_names.contains_key(&g.root_id(p))).unwrap();
        let again = g.object_states_of(&[pasted], [0.0; 3]);
        assert_eq!(types(&again[0]), t);
        // The reference fields name the object's own identity, which the second read regenerates: compare the names and types only.
        let strip = |s: &ObjectState| -> Vec<MonoBehaviourPrototype> {
            let mut v: Vec<_> = s.RootComponentData.iter().filter(|m| m.Type != "SerialisableIdentity").cloned().collect();
            for m in &mut v {
                m.storedReferenceFields.iter_mut().for_each(|(_, l)| l.UniqueIdentity = 0);
            }
            v
        };
        assert!(physical.storedReferenceFields.iter().any(|(n, l)| n == "ActivationNode" && l.ComponentType.as_deref() == Some("Activations.NodeBehaviour")));
        // The game differs here too: Copy packs the selected object (ShowOutline true), and the paste's PhysicalBehaviour.Start sets
        // ShowOutline = false after Reconstruct injected it; PasteAction selects nothing. Every other member survives the paste.
        let outline = |s: &ObjectState| s.RootComponentData.iter().find(|m| m.Type == "PhysicalBehaviour").and_then(|m| m.storedProperties.iter().find(|(n, _)| n == "ShowOutline").map(|(_, v)| v.clone()));
        assert_eq!((outline(&clip.States[0]), outline(&again[0])), (Some(SavedValue::Field("1".into())), Some(SavedValue::Field("0".into()))));
        let no_outline = |s: &ObjectState| -> Vec<MonoBehaviourPrototype> {
            let mut v = strip(s);
            v.iter_mut().for_each(|m| m.storedProperties.retain(|(n, _)| n != "ShowOutline"));
            v
        };
        assert_eq!(no_outline(&again[0]), no_outline(&clip.States[0]));
        assert_eq!(again[0].Root.LocalScale, clip.States[0].Root.LocalScale);
        // The pasted object has the clipboard's identity, the original a new one from the copy.
        let identity = |s: &ObjectState| s.RootComponentData.iter().find(|m| m.Type == "SerialisableIdentity").cloned();
        assert!(identity(&clip.States[0]).is_some());
        g.undo();
        assert!(g.notifications.last().is_some_and(|n| n == "<b>PASTE</b> removed"));
    }

    /// The context menu's Copy and Paste buttons: the clipboard filled without the "Copied" note, then a PasteLoadAction named "Paste".
    #[test]
    fn context_menu_copy_and_paste() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut g = new_game(&game, "Default", Some("Brick"), None, 0).unwrap();
        frames(&mut g, 30);
        let first = g.physicals.len();
        let c = g.BoundingBox.center;
        let Some(hit) = g.raycast_first(crate::b2::math::Vec2::new(c[0], c[1]), [0.0, -1.0], 1.0e4) else { return };
        g.input.mouse = [c[0], hit.point.y + 0.6, 0.0];
        ctl(&mut g, "spawnRight", false, true);
        frames(&mut g, 1);
        frames(&mut g, 300);
        assert!(g.physicals.len() > first);
        // Nothing selected: Copy leaves the menu open and nothing copied; Paste with nothing copied does the same.
        assert!(!g.context_static_click("Copy", [c[0], c[1]]) && g.clipboard.is_none());
        assert!(!g.context_static_click("Paste", [c[0], c[1]]));
        g.select(first, false);
        let notes = g.notifications.len();
        assert!(g.context_static_click("Copy", [c[0], c[1]]));
        assert_eq!((g.clipboard.as_ref().map(|c| c.Count()), g.notifications.len()), (Some(1), notes));
        let count = g.physicals.len();
        assert!(g.context_static_click("Paste", [c[0] + 4.0, c[1]]));
        frames(&mut g, 1);
        assert!(g.physicals.len() > count);
        assert!(matches!(g.undo.history.last(), Some(super::super::undo::UndoAction::Paste(a)) if a.displayName == "Paste"));
    }

    /// Every catalog item spawned, settled, copied and pasted: lists the items whose ObjectState still reaches a stop (a script class
    /// `script_ref` does not know, ...). Slow, so run by hand: `cargo test copy_paste_every_catalog_item -- --ignored --nocapture`.
    #[test]
    fn copy_paste_every_catalog_item() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let catalog = db.find_scripts("CatalogData").unwrap()[0];
        let (_, _, cat) = db.mono(catalog).unwrap().unwrap();
        let mut names = Vec::new();
        for item in cat.get("Items").and_then(crate::assets::mono::Value::array).unwrap().to_vec() {
            let asset = db.resolve(catalog.file, item.pptr().unwrap()).unwrap();
            let (_, mb, sp) = db.mono(asset).unwrap().unwrap();
            if let Some(p) = sp.get("Prefab").and_then(crate::assets::mono::Value::pptr).and_then(|p| db.resolve(asset.file, p)) {
                names.push(mb.name.clone());
            }
        }
        // The items run on 8 threads (each its own Game); a 30 s watchdog per thread prints HANG <item>.
        let failures = std::sync::Mutex::new(Vec::new());
        let gaps = std::sync::Mutex::new(std::collections::BTreeSet::new());
        let only = std::env::var("CATALOG_ONLY").ok();
        let names: Vec<&String> = names.iter().filter(|n| only.as_ref().is_none_or(|o| o.split(';').any(|x| x == n.as_str()))).collect();
        let next = std::sync::atomic::AtomicUsize::new(0);
        let current: Vec<_> = (0..8).map(|_| std::sync::Arc::new(std::sync::Mutex::new((String::new(), std::time::Instant::now(), false)))).collect();
        let watched = current.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            for w in &watched {
                let c = w.lock().unwrap();
                if c.2 && c.1.elapsed().as_secs() > 30 {
                    eprintln!("HANG {}", c.0);
                    std::process::exit(3);
                }
            }
        });
        std::thread::scope(|s| {
            for slot in &current {
                let (game, names, next, failures, gaps) = (&game, &names, &next, &failures, &gaps);
                std::thread::Builder::new().stack_size(64 << 20).spawn_scoped(s, move || loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(name) = names.get(i) else { slot.lock().unwrap().2 = false; break };
                    eprintln!("ITEM {name}");
                    *slot.lock().unwrap() = (name.to_string(), std::time::Instant::now(), true);
                    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut g = new_game(game, "Default", Some(name.as_str()), None, 0).unwrap();
                        frames(&mut g, 30);
                        let first = g.physicals.len();
                        let c = g.BoundingBox.center;
                        let Some(hit) = g.raycast_first(crate::b2::math::Vec2::new(c[0], c[1]), [0.0, -1.0], 1.0e4) else { return };
                        g.input.mouse = [c[0], hit.point.y + 0.6, 0.0];
                        ctl(&mut g, "spawnRight", false, true);
                        frames(&mut g, 1);
                        if g.physicals.len() <= first {
                            return;
                        }
                        frames(&mut g, 120);
                        g.select(first, false);
                        ctl(&mut g, "copy", true, false);
                        frames(&mut g, 1);
                        g.input.mouse = [c[0] + 4.0, hit.point.y + 0.6, 0.0];
                        ctl(&mut g, "paste", false, true);
                        frames(&mut g, 2);
                        if !g.save_gaps.is_empty() {
                            eprintln!("GAPITEM {name}: {}", g.save_gaps.iter().map(|s| s.split(' ').next().unwrap_or("")).collect::<Vec<_>>().join(" "));
                        }
                        gaps.lock().unwrap().extend(g.save_gaps.iter().cloned());
                    }));
                    if let Err(e) = run {
                        let m = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                        failures.lock().unwrap().push(format!("{name}: {m}"));
                    }
                }).unwrap();
            }
        });
        let (failures, gaps) = (failures.into_inner().unwrap(), gaps.into_inner().unwrap());
        let mut report = String::new();
        for g in gaps.iter() {
            report += &format!("GAP {g}
");
        }
        for f in &failures {
            report += &format!("FAIL {f}
");
        }
        if let Ok(dir) = std::env::var("CARGO_TARGET_DIR") {
            let _ = std::fs::write(std::path::Path::new(&dir).join("catalog-copy-paste.txt"), &report);
        }
        eprint!("{report}");
        // Members whose live value is presentation state the rewrite does not model yet (a video clock, a runtime
        // component's colour, the explosion codec type): still open.
        const OPEN: &[&str] = &["TelevisionBehaviour.videoTime", "PseudoBloodImpactHelper.Color", "MachineGunBehaviour.ExplosionSettings"];
        let open: Vec<_> = gaps.iter().filter(|g| !OPEN.iter().any(|o| g.starts_with(o))).cloned().collect();
        assert!(open.is_empty(), "ObjectState gaps: {open:?}");
        assert!(failures.is_empty(), "{} of {} items stop on copy and paste", failures.len(), names.len());
    }


    /// The first catalog item whose root object has no PhysicalBehaviour, no Rigidbody2D, and no collider a body added to it
    /// would take (every collider sits on or under another Rigidbody2D).
    fn bodyless_root_item(db: &mut AssetDb) -> String {
        use crate::assets::db::Component;
        let catalog = db.find_scripts("CatalogData").unwrap()[0];
        let (_, _, cat) = db.mono(catalog).unwrap().unwrap();
        for item in cat.get("Items").and_then(crate::assets::mono::Value::array).unwrap().to_vec() {
            let asset = db.resolve(catalog.file, item.pptr().unwrap()).unwrap();
            let (_, mb, sp) = db.mono(asset).unwrap().unwrap();
            let Some(prefab) = sp.get("Prefab").and_then(crate::assets::mono::Value::pptr).and_then(|p| db.resolve(asset.file, p)) else { continue };
            let p = db.prefab_cached(prefab).unwrap();
            let has = |o: usize, f: &dyn Fn(&Component) -> bool| p.objects[o].components.iter().any(|(_, c)| f(c));
            let body = |o: usize| has(o, &|c| matches!(c, Component::Rigidbody2D(_)));
            let physical = |o: usize| has(o, &|c| matches!(c, Component::Script { class, .. } if class == "PhysicalBehaviour"));
            if physical(0) || body(0) || !(1..p.objects.len()).any(physical) {
                continue;
            }
            let covered = |o: usize| {
                let mut up = Some(o);
                while let Some(x) = up.filter(|&x| x != 0) {
                    if body(x) {
                        return true;
                    }
                    up = p.objects[x].parent;
                }
                false
            };
            if (0..p.objects.len()).all(|o| !has(o, &|c| matches!(c, Component::Collider2D(_))) || covered(o)) {
                return mb.name.clone();
            }
        }
        panic!("no catalog item has a root without a PhysicalBehaviour");
    }

    /// A hand-built state: a catalog item whose root has no PhysicalBehaviour and a Brick, packed together, the tool component
    /// `class` (stored members `fields`) added to the item root's data with its PhysicalBehaviour reference `other` naming the
    /// Brick, then converted 8 units to the right and run 3 frames. The new root's tool component id and the pasted Brick.
    fn root_tool(class: &str, fields: &[(&str, String)], other: &str) -> Option<(Game, i32, usize)> {
        let Ok(game) = crate::game_assets::find_game(None) else { return None };
        let item = bodyless_root_item(&mut AssetDb::open(&game).unwrap());
        let mut g = new_game(&game, "Default", Some(&item), None, 0).unwrap();
        frames(&mut g, 30);
        let c = g.BoundingBox.center;
        let hit = g.raycast_first(crate::b2::math::Vec2::new(c[0], c[1]), [0.0, -1.0], 1.0e4).expect("the floor");
        let spawn = |g: &mut Game, x: f32| {
            let first = g.physicals.len();
            g.input.mouse = [c[0] + x, hit.point.y + 3.0, 0.0];
            ctl(g, "spawnRight", false, true);
            frames(g, 1);
            assert!(g.physicals.len() > first);
            first
        };
        let a = spawn(&mut g, -6.0);
        g.catalog_selected = Some("Brick".into());
        let b = spawn(&mut g, -3.0);
        frames(&mut g, 10);
        let mut states = g.object_states_of(&[a, b], [0.0; 3]);
        let brick = g.identity_of(g.root_id(b));
        let s = states.iter_mut().find(|s| s.SpawnableAssetName == item).unwrap();
        s.RootComponentData.push(MonoBehaviourPrototype {
            Enabled: true,
            Type: class.into(),
            storedFields: fields.iter().map(|(n, v)| (n.to_string(), SavedValue::Field(v.clone()))).collect(),
            storedProperties: Vec::new(),
            storedReferenceFields: vec![(other.into(), ComponentReferenceLink { UniqueIdentity: brick, ComponentType: Some("PhysicalBehaviour".into()) })],
        });
        let count = g.physicals.len();
        g.object_states_convert(&states, [8.0, 0.0, 0.0], false);
        frames(&mut g, 3);
        let pasted = (count..g.physicals.len()).find(|&p| g.physicals[p].name.starts_with("Brick")).expect("the pasted Brick");
        let id = g.root_wires.last().map(|w| w.id).into_iter().chain(g.pins.last().map(|p| p.id)).chain(g.phase_links.last().map(|l| l.id)).max().unwrap();
        Some((g, id, pasted))
    }

    /// The root wire `id` of a `root_tool` game: started, its joint on the root's new Rigidbody2D and connected to the Brick, alive.
    fn root_wire(g: &Game, id: i32, brick: usize) -> (super::super::blood_wire::RootWire, usize) {
        let w = g.root_wires.iter().find(|w| w.id == id).unwrap().clone();
        let f = w.joint;
        assert!(w.started && w.usedToHaveConnectedBody && w.other == Some(brick) && !g.destroyed_components.contains(&id), "{w:?}");
        assert!(matches!(g.fixed_joints[f].attached, BodyOwner::Node(k) if k == w.body) && g.fixed_joints[f].connected == Some(BodyOwner::Physical(brick)));
        assert_eq!(w.line, w.vertices);
        (w, f)
    }

    /// A CableBehaviour on a root: a DistanceJoint2D from the stored members, Update's 13-point parabola from the joint's ends,
    /// Tick's currentDistance kept at the joint's.
    #[test]
    fn a_cable_on_an_object_without_a_physical_behaviour() {
        let Some((g, id, brick)) = root_tool("CableBehaviour", &[("currentDistance", 1.5f32.show()), ("WireWidth", 0.1f32.show())], "otherPhysicalBehaviour") else { return };
        let (w, f) = root_wire(&g, id, brick);
        let d = g.fixed_joints[f].distance.unwrap();
        assert!(d.auto_configure_distance && w.saved.currentDistance == d.distance);
        assert_eq!(w.line.len(), 13);
        let (a, b) = g.joint_ends(f);
        assert!((0..2).all(|i| (w.line[0][i] - a[i]).abs() < 1e-4 && (w.line[12][i] - b[i]).abs() < 1e-4), "{:?} {a:?} {b:?}", w.line);
    }

    /// A SpringCableBehaviour on a root: a SpringJoint2D at targetDistance with the default frequency and damping (Start's
    /// SpringStrength comes after Initialise), max(2, ceil(distance)) points, and Tick's burn on initialBreakForce.
    #[test]
    fn a_spring_cable_on_an_object_without_a_physical_behaviour() {
        let Some((g, id, brick)) = root_tool("SpringCableBehaviour", &[("targetDistance", 3.5f32.show()), ("currentBreakingForce", 100f32.show()), ("SpringStrength", 7f32.show())], "otherPhysicalBehaviour") else { return };
        let (w, f) = root_wire(&g, id, brick);
        let s = g.fixed_joints[f].spring.unwrap();
        assert_eq!((s.distance, s.frequency, s.damping_ratio, s.auto_configure_distance), (3.5, 1.0, 0.0, false));
        assert_eq!((w.line.len(), w.initialBreakForce, g.fixed_joints[f].state.break_force), (4, 100.0, 100.0));
    }

    /// A SliderJointWireBehaviour on a root: a SliderJoint2D and Update's three points.
    #[test]
    fn a_slider_on_an_object_without_a_physical_behaviour() {
        let Some((g, id, brick)) = root_tool("SliderJointWireBehaviour", &[], "otherPhysicalBehaviour") else { return };
        let (w, f) = root_wire(&g, id, brick);
        assert!(g.fixed_joints[f].slider.is_some());
        assert_eq!(w.line.len(), 3);
    }

    /// A breakable UseWireBehaviour and a WoodStrutBehaviour on a root: Tick's burn sets breakForce to the initial
    /// breakForceBuffer (1); the strut's Created made it white.
    #[test]
    fn burning_wires_on_an_object_without_a_physical_behaviour() {
        let Some((g, id, brick)) = root_tool("UseWireBehaviour", &[("CanBreak", true.show()), ("Channel", 2u16.show())], "otherPhysicalBehaviour") else { return };
        let (w, f) = root_wire(&g, id, brick);
        assert_eq!((w.kind, g.fixed_joints[f].state.break_force, w.line.len()), (super::super::blood_wire::WireKind::Use(2, true), 1.0, 13));
        let Some((g, id, brick)) = root_tool("WoodStrutBehaviour", &[("WireColor", [0.5f32, 0.2, 0.1, 1.0].show())], "otherPhysicalBehaviour") else { return };
        let (w, f) = root_wire(&g, id, brick);
        assert_eq!((w.WireColor, w.start_color, g.fixed_joints[f].state.break_force, w.line.len()), ([1.0; 4], [1.0; 4], 1.0, 2));
    }

    /// The blood wire and heat pipe on a root: a DistanceJoint2D and the line, nothing else (their Ticks need physicalBehaviour).
    #[test]
    fn blood_wire_and_heat_pipe_on_an_object_without_a_physical_behaviour() {
        for (class, points) in [("BloodWireBehaviour", 13), ("HeatPipeBehaviour", 2)] {
            let Some((g, id, brick)) = root_tool(class, &[("currentDistance", 2f32.show())], "otherPhysicalBehaviour") else { return };
            let (w, f) = root_wire(&g, id, brick);
            assert_eq!((w.line.len(), g.fixed_joints[f].state.break_force), (points, f32::INFINITY), "{class}");
        }
    }

    /// A PinBehaviour on a root works: a HingeJoint2D on the root's new Rigidbody2D connected to the Brick, its sprite and tracker.
    #[test]
    fn a_pin_on_an_object_without_a_physical_behaviour() {
        let Some((mut g, id, brick)) = root_tool("PinBehaviour", &[("BreakingThreshold", 10f32.show())], "Other") else { return };
        let p = g.pins.iter().position(|p| p.id == id).unwrap();
        let x = g.pins[p].clone();
        assert!(x.started && x.root.is_some() && x.renderer.is_some() && !g.destroyed_components.contains(&id));
        let f = x.Joint.unwrap();
        assert!(g.fixed_joints[f].hinge.is_some() && matches!(g.fixed_joints[f].attached, BodyOwner::Node(_)) && g.fixed_joints[f].connected == Some(BodyOwner::Physical(brick)));
        assert!(x.initialBreakForce.is_finite() && g.pin_host_active(p));
        // The sprite sits at the root's TransformPoint(LocalAnchor) (0, 0): the root's position.
        let at = g.renderers[x.renderer.unwrap()].chain[0].t;
        let root = g.pin_host_pose(p).0;
        assert!((at[0] - root[0]).abs() < 1e-3 && (at[1] - root[1]).abs() < 1e-3, "{at:?} {root:?}");
    }

    /// A PhaseLinkBehaviour on a root: Start throws in PrepareSpriteRendererOrder, and the first unpaused Update destroys it.
    #[test]
    fn a_phase_link_on_an_object_without_a_physical_behaviour() {
        let Some((g, id, _)) = root_tool("PhaseLinkBehaviour", &[], "Other") else { return };
        let l = g.phase_links.iter().find(|l| l.id == id).unwrap();
        assert!(l.start_threw && !l.started && g.destroyed_components.contains(&id));
    }
}

/// The tool kind a wire class stands for (the spring cables differ only in WireMaterial and SpringStrength).
fn wire_kind(class: &str, material: &str, channel: u16, can_break: bool) -> super::blood_wire::WireKind {
    use super::blood_wire::WireKind;
    match class {
        "BloodWireBehaviour" => WireKind::Blood,
        "CableBehaviour" => WireKind::Cable,
        "RopeBehaviour" => WireKind::Rope,
        "ChainBehaviour" => WireKind::Chain,
        "CopperWireBehaviour" => WireKind::Copper,
        "EnergyWireBehaviour" => WireKind::Energy,
        "HeatPipeBehaviour" => WireKind::HeatPipe,
        "RigidWireBehaviour" => WireKind::Rigid,
        "SpringCableBehaviour" => match material {
            "StrongSpringCable" => WireKind::StrongSpring,
            "DestructibleSpringCable" => WireKind::DestructibleSpring,
            _ => WireKind::Spring,
        },
        "AppliedBandageBehaviour" => WireKind::Bandage,
        "WoodStrutBehaviour" => WireKind::WoodStrut,
        "FixedCableBehaviour" => WireKind::FixedCable,
        "SteelBindingBehaviour" => WireKind::SteelBinding,
        "WoodenBindingBehaviour" => WireKind::WoodenBinding,
        "SliderJointWireBehaviour" => WireKind::Slider,
        "UseWireBehaviour" => WireKind::Use(channel, can_break),
        _ => unreachable!("{class} is a tool component"),
    }
}

/// GameObject.tag: Unity's builtin tags, then TagManager's (globalgamemanagers m_Tags, user tags from 20000).
fn tag_name(tag: u16) -> String {
    const BUILTIN: [&str; 8] = ["Untagged", "Respawn", "Finish", "EditorOnly", "", "MainCamera", "Player", "GameController"];
    const USER: [&str; 12] = ["Flammable", "Limb", "NoClear", "Map", "QuickAccess", "RecursiveLayerChild", "HeatEffect", "ColdEffect", "Poolable", "NoSoundButton", "nitro", "World"];
    match tag {
        t if (t as usize) < BUILTIN.len() => BUILTIN[t as usize],
        t if t >= 20000 => USER.get((t - 20000) as usize).copied().unwrap_or("Untagged"),
        _ => "Untagged",
    }
    .to_string()
}
