//! 1.28.45's new classes: SpeedometerBehaviour (and SpeedUnit, SpeedUnitDisplayAttribute, EnumDisplayAttribute),
//! CrossbowBehaviour, InjectableBehaviour, BuzzerBehaviour, DulcamarusBehaviour (with SiphonSource),
//! DrippingBloodParticleController and CircularBuffer`1. Everything here exists only with 1.28.45 installed
//! (`il::is_1_28_45`); with 1.28.41 no prefab carries these classes and no liquid adds them.
#![allow(non_snake_case)]

use super::container::Owner;
use super::{mathf, Behaviour, Game};
use crate::assets::db::{Component, ObjRef, Prefab};
use crate::assets::mono::Value;

/// The new classes the link step builds from a prefab.
pub const LINKED: &[&str] = &["SpeedometerBehaviour", "CrossbowBehaviour", "BuzzerBehaviour", "DrippingBloodParticleController", "MagneticBehaviour"];

/// The Unity messages each new class declares (lifecycle.rs is generated from the 1.28.41 DLL, which lacks them).
pub const LIFECYCLE: &[(&str, &[&str])] = &[
    ("SpeedometerBehaviour", &["Start", "OnEnable", "FixedUpdate", "OnWillRenderObject", "OnDisable"]),
    ("CrossbowBehaviour", &["Awake", "OnCollisionEnter2D", "Update", "OnJointBreak2D", "OnDestroy"]),
    ("BuzzerBehaviour", &["Awake", "Start", "OnWillRenderObject"]),
    ("DulcamarusBehaviour", &["Awake", "Start", "Update", "OnDestroy"]),
    ("InjectableBehaviour", &["Start"]),
    ("MagneticBehaviour", &["Awake", "FixedUpdate"]),
    ("DrippingBloodParticleController", &["Update"]),
];

/// The new classes that take Use (Messages.IUse).
pub const USE_CLASSES: &[&str] = &["SpeedometerBehaviour", "BuzzerBehaviour"];

/// A new behaviour by its `V45` arena index.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum V {
    Speedometer(usize),
    Crossbow(usize),
    Buzzer(usize),
    Dulcamarus(usize),
    DrippingBlood(usize),
    Magnetic(usize),
    /// InjectableBehaviour: no manager callbacks (Start is rendering, Lodged a message); its save slot only.
    Injectable(usize),
}

impl V {
    pub fn class(self) -> &'static str {
        match self {
            V::Speedometer(_) => "SpeedometerBehaviour",
            V::Crossbow(_) => "CrossbowBehaviour",
            V::Buzzer(_) => "BuzzerBehaviour",
            V::Dulcamarus(_) => "DulcamarusBehaviour",
            V::DrippingBlood(_) => "DrippingBloodParticleController",
            V::Magnetic(_) => "MagneticBehaviour",
            V::Injectable(_) => "InjectableBehaviour",
        }
    }
}

/// SpeedUnit
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum SpeedUnit {
    #[default]
    MetersPerSecond,
    KilometersPerHour,
    FeetPerSecond,
    MilesPerHour,
}

impl SpeedUnit {
    /// The enum's value (Preferences.SpeedUnit as an int; another value matches no case and reads as m/s here).
    pub fn from_index(i: i32) -> Self {
        match i {
            1 => SpeedUnit::KilometersPerHour,
            2 => SpeedUnit::FeetPerSecond,
            3 => SpeedUnit::MilesPerHour,
            _ => SpeedUnit::MetersPerSecond,
        }
    }

    /// SpeedUnitDisplayAttribute.GetDisplay (EnumDisplayAttribute.GetDisplay's override): the settings radio label.
    pub fn display(self) -> &'static str {
        match self {
            SpeedUnit::MetersPerSecond => crate::il45!(str "SpeedUnitDisplayAttribute::GetDisplay" 0),
            SpeedUnit::KilometersPerHour => crate::il45!(str "SpeedUnitDisplayAttribute::GetDisplay" 1),
            SpeedUnit::FeetPerSecond => crate::il45!(str "SpeedUnitDisplayAttribute::GetDisplay" 2),
            SpeedUnit::MilesPerHour => crate::il45!(str "SpeedUnitDisplayAttribute::GetDisplay" 3),
        }
    }
}

/// CircularBuffer`1
#[derive(Clone, Debug)]
pub struct CircularBuffer<T: Copy + Default> {
    pub Data: Vec<T>,
    pub WriteIndex: i32,
}

impl<T: Copy + Default> CircularBuffer<T> {
    /// CircularBuffer`1..ctor(size)
    pub fn new(size: usize) -> Self {
        CircularBuffer { Data: vec![T::default(); size], WriteIndex: 0 }
    }
    /// CircularBuffer`1.Add
    pub fn add(&mut self, value: T) {
        let n = self.Data.len() as i32;
        self.Data[(self.WriteIndex % n) as usize] = value;
        self.WriteIndex += 1;
    }
    /// CircularBuffer`1.PickRandom: Random.Range(0, WriteIndex - 1) % Length (default before the first Add).
    pub fn pick_random(&self, g: &mut Game) -> T {
        if self.WriteIndex == 0 {
            return T::default();
        }
        let k = g.random.range_int(0, self.WriteIndex - 1);
        self.Data[(k % self.Data.len() as i32) as usize]
    }
}

/// SpeedometerBehaviour
#[derive(Clone, Debug, Default)]
pub struct Speedometer {
    pub id: i32,
    pub enabled: bool,
    pub Phys: usize,
    pub Activated: bool,
    pub TopSpeedMs: f32,
    pub speedMs: f32,
    /// Light (its SpriteRenderer) and TextMesh (a `Game::world_texts` index); None: null.
    pub Light: Option<super::visual::SpriteAt>,
    pub TextMesh: Option<usize>,
}

/// CrossbowBehaviour
#[derive(Clone, Debug)]
pub struct Crossbow {
    pub id: i32,
    pub enabled: bool,
    /// Launcher (`Game::launchers`); its PhysicalBehaviour is the crossbow's own.
    pub Launcher: usize,
    pub phys: usize,
    pub LocalSyringeAttachPoint: [f32; 2],
    pub SyringeEjectForce: f32,
    pub SyringeFluidUsage: f32,
    /// AttachedSyringe (`Game::syringes`).
    pub AttachedSyringe: Option<usize>,
    /// syringeJoint (`Game::fixed_joints`).
    pub syringeJoint: Option<usize>,
    pub lastDisconnectTime: f32,
    pub hasSyringe: bool,
    /// The component's index on its object and whether it comes after the PhysicalBehaviour (collision order).
    pub index: usize,
    pub after_physical: bool,
    /// ImbueEffect: playing, and main.startColor / emission.enabled (rendering).
    pub imbue_playing: bool,
    pub imbue_color: [f32; 4],
    pub imbue_emission: bool,
}

/// InjectableBehaviour (added at runtime to a launched bolt).
#[derive(Clone, Debug, Default)]
pub struct Injectable {
    pub id: i32,
    /// The bolt's root PhysicalBehaviour (Lodged arrives by SendMessage to its GameObject).
    pub phys: Option<usize>,
    /// LiquidDistributions: (LiquidID, Amount); None once spent (null).
    pub LiquidDistributions: Option<Vec<(String, f32)>>,
    pub FxColor: [f32; 4],
    pub destroyed: bool,
    /// ImbueParticles: the `Game::embedded_ps` system Start found (None: null).
    pub ImbueParticles: Option<usize>,
}

/// BuzzerBehaviour
#[derive(Clone, Debug, Default)]
pub struct Buzzer {
    pub id: i32,
    pub enabled: bool,
    pub PhysicalBehaviour: usize,
    /// DamagableBehaviour (`Game::machinery`).
    pub DamagableBehaviour: Option<usize>,
    pub AudioData: String,
    pub CurrentDataId: u32,
    pub Loudness: f32,
    pub lastPlayedTime: f32,
    /// Source.minDistance / maxDistance (UpdateLoudness).
    pub min_distance: f32,
    pub max_distance: f32,
    /// SerialisableAudioData (null outside Start and after OnBeforeSerialise).
    pub SerialisableAudioData: Option<Vec<u8>>,
    /// clip (ClipFromData's; None: the FallbackAudioClip plays).
    pub clip: Option<super::audio::ClipRef>,
    /// SpriteRenderer.color (UpdateColour).
    pub color: [f32; 4],
    /// SpriteRenderer, and the Diaphragm Transform's own SpriteRenderer (OnWillRenderObject scales it); None: null.
    pub SpriteRenderer: Option<super::visual::SpriteAt>,
    pub Diaphragm: Option<super::visual::SpriteAt>,
}

/// DulcamarusBehaviour.SiphonSource
#[derive(Clone, Debug)]
pub struct SiphonSource {
    pub Limb: usize,
    pub SourcePos: [f32; 3],
    pub TargetPos: [f32; 3],
    pub Time: f32,
    pub Strength: f32,
    pub Seed: f32,
    /// LineRenderer ("tendril"), drawn from `Game::lines`.
    pub LineRenderer: Option<Tendril>,
}

#[derive(Clone, Debug, Default)]
pub struct Tendril {
    /// The LineRenderer's instance id.
    pub id: i32,
    /// Its index in `Game::lines`.
    pub line: usize,
}

/// DulcamarusBehaviour (added at runtime to a person by the Dulcamarus liquid).
#[derive(Clone, Debug)]
pub struct Dulcamarus {
    pub id: i32,
    pub Person: usize,
    pub Radius: f32,
    pub isNew: bool,
    pub head: Option<usize>,
    pub indexByLimb: std::collections::HashMap<usize, usize>,
    pub siphonSources: [Option<SiphonSource>; 16],
    pub shouldHeal: bool,
    pub distributor: super::misc::FixedIntervalDistributor,
    pub destroyed: bool,
}

/// DrippingBloodParticleController
#[derive(Clone, Debug)]
pub struct DrippingBlood {
    pub id: i32,
    pub PhysicalBehaviour: Option<usize>,
    pub Colour: [f32; 4],
    pub Amount: f32,
    pub DecaySpeed: f32,
    pub LocalDripAreas: CircularBuffer<[f32; 2]>,
    pub clock: super::misc::FixedIntervalDistributor,
    /// ParticleSystem.Emit(params, 1) calls: (position, colour) (rendering).
    pub emitted: Vec<([f32; 2], [f32; 4])>,
}

/// MagneticBehaviour (no 1.28.45 prefab carries it; a mod's or a saved one's).
#[derive(Clone, Debug, Default)]
pub struct Magnetic {
    pub id: i32,
    /// rb: GetComponent<Rigidbody2D>() in Awake, the physical on its object.
    pub phys: usize,
    pub EffectiveDistance: f32,
    pub NorthPole: [f32; 2],
    pub SouthPole: [f32; 2],
    pub MagnetStrength: f32,
    pub globalNorthPole: [f32; 2],
    pub globalSouthPole: [f32; 2],
}

/// MagneticBehaviour.Pole
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Pole {
    North,
    South,
}

/// The new classes' arenas.
#[derive(Default)]
pub struct V45 {
    pub speedometers: Vec<Speedometer>,
    pub crossbows: Vec<Crossbow>,
    pub injectables: Vec<Injectable>,
    pub buzzers: Vec<Buzzer>,
    pub dulcamarus: Vec<Dulcamarus>,
    pub drips: Vec<DrippingBlood>,
    pub magnets: Vec<Magnetic>,
    /// BuzzerBehaviour.DataCache and BuzzerBehaviour.ClipCache (statics: kept for the whole run).
    pub buzzer_data_cache: std::collections::HashMap<u32, Vec<u8>>,
    pub buzzer_clip_cache: std::collections::HashMap<u32, super::audio::ClipRef>,
    /// Global.DrippingParticlesPrefab: each object's (ids it takes, parent), the controller's (object, component), and
    /// its Amount and DecaySpeed, and its ParticleSystem field's (GameObject, Transforms up to the clone root).
    pub drip_prefab: Option<DripPrefab>,
    /// `Game::lines` slots of destroyed tendrils, for the next CreateLineRenderer.
    pub free_tendrils: Vec<usize>,
}

type DripPrefab = (Vec<(usize, Option<usize>)>, (usize, usize), f32, f32, Option<(ObjRef, Vec<crate::unity::transform::Trs>)>);

/// A script's PPtr field `name` as the prefab object it points at (None: null).
fn field_object(prefab: &Prefab, o: usize, v: &Value, name: &str) -> Option<usize> {
    field_component(prefab, o, v, name).map(|(k, _)| k)
}

/// A script's PPtr field `name` as the prefab (object, component) it points at (None: null).
fn field_component(prefab: &Prefab, o: usize, v: &Value, name: &str) -> Option<(usize, usize)> {
    let p = v.get(name)?.pptr()?;
    if p.path_id == 0 {
        return None;
    }
    let file = prefab.objects[o].game_object.file;
    prefab.index.get(&ObjRef { file, path_id: p.path_id }).copied().filter(|_| p.file == 0)
}

fn f(v: &Value, name: &str, default: f32) -> f32 {
    v.get(name).and_then(Value::f32).unwrap_or(default)
}

fn vec2(v: &Value, name: &str) -> [f32; 2] {
    v.get(name).map_or([0.0; 2], |x| [f(x, "x", 0.0), f(x, "y", 0.0)])
}

/// Color.grayscale
fn grayscale(c: [f32; 4]) -> f32 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}

/// `{x:N1}` of a float: 7 significant digits (Single.ToString), then one decimal rounded half away from zero, with
/// thousands separators.
pub fn format_n1(x: f32) -> String {
    let s = format!("{:.6e}", x as f64);
    let (mantissa, exp) = s.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let negative = mantissa.starts_with('-');
    let digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
    // digits[0] is at 10^exp; keep down to 10^-1.
    let keep = (exp + 2).max(0) as usize;
    let mut kept: Vec<u8> = (0..keep).map(|i| digits.get(i).copied().unwrap_or(0)).collect();
    if digits.get(keep).is_some_and(|&d| d >= 5) {
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, 1);
                break;
            }
            i -= 1;
            if kept[i] == 9 {
                kept[i] = 0;
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    while kept.len() < 2 {
        kept.insert(0, 0);
    }
    let (int, frac) = kept.split_at(kept.len() - 1);
    let mut int: String = int.iter().map(|d| (b'0' + d) as char).collect();
    let trimmed = int.trim_start_matches('0');
    int = if trimmed.is_empty() { "0".into() } else { trimmed.into() };
    let mut grouped = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    let zero = int == "0" && frac[0] == 0;
    format!("{}{grouped}.{}", if negative && !zero { "-" } else { "" }, frac[0])
}

/// MagneticBehaviour.GetForce
pub(crate) fn magnetic_get_force(MagnetStrength: f32, a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    let val = [b[0] - a[0], b[1] - a[1]];
    // Vector2.sqrMagnitude: one double-precision expression (Mono), rounded on return.
    let sqr = super::mathf::sqr_magnitude(val);
    if sqr < 0.01 {
        return [0.0, 0.0];
    }
    let num2 = MagnetStrength / (sqr + 0.01);
    [val[0] * num2, val[1] * num2]
}

/// MagneticBehaviour.GetPoleSign
pub(crate) fn magnetic_get_pole_sign(a: Pole, b: Pole) -> f32 {
    if a == b {
        -1.0
    } else {
        1.0
    }
}

/// SpeedometerBehaviour.ConvertSpeed: switches on the preference, not on `toUnit`; km/h reads "km/s" (the game's text).
pub fn convert_speed(metersPerSecond: f32, unit: SpeedUnit) -> String {
    let (v, suffix) = match unit {
        SpeedUnit::KilometersPerHour => (metersPerSecond * 3.6, crate::il45!(str "SpeedometerBehaviour::ConvertSpeed" 0)),
        SpeedUnit::FeetPerSecond => (metersPerSecond * 3.281, crate::il45!(str "SpeedometerBehaviour::ConvertSpeed" 1)),
        SpeedUnit::MilesPerHour => (metersPerSecond * 2.237, crate::il45!(str "SpeedometerBehaviour::ConvertSpeed" 2)),
        SpeedUnit::MetersPerSecond => (metersPerSecond, crate::il45!(str "SpeedometerBehaviour::ConvertSpeed" 3)),
    };
    // $"{x:N1} km/s": the format string's literal tail after the hole.
    let tail = suffix.split_once('}').map_or(suffix, |(_, t)| t);
    format!("{}{tail}", format_n1(mathf::clamp(v, 0.0, 9999.0)))
}

impl Game {
    /// The link step for a new class's component `c` on object `o` (Awake included), and its manager entry.
    pub(crate) fn v45_link(&mut self, prefab: &Prefab, inst: &super::spawn::Instance, linked: &super::link::Linked, o: usize, c: usize) -> Option<Behaviour> {
        let Component::Script { class, enabled, fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
        let id = inst.objects[o].components[c];
        let enabled = *enabled;
        let physical_at = |k: Option<usize>| k.and_then(|k| linked.physicals.get(&k).copied());
        let b = match class.as_str() {
            "SpeedometerBehaviour" => {
                let Phys = physical_at(field_object(prefab, o, v, "Phys")).unwrap_or_else(|| unimplemented!("mod-only: SpeedometerBehaviour.Phys null"));
                let Light = field_object(prefab, o, v, "Light").map(|k| self.v45_sprite_at(inst, linked, prefab, k));
                // TextMesh: the TextMeshPro, as link.rs's text_field finds it.
                let TextMesh = field_component(prefab, o, v, "TextMesh").map(|(k, kc)| {
                    let tid = inst.objects[k].components[kc];
                    self.world_texts.iter().position(|t| t.id == tid).unwrap_or_else(|| unimplemented!("mod-only: SpeedometerBehaviour.TextMesh: not a TextMeshPro"))
                });
                self.v45.speedometers.push(Speedometer { id, enabled, Phys, Activated: v.get("Activated").and_then(Value::bool).unwrap_or(true), TopSpeedMs: f(v, "TopSpeedMs", 0.0), Light, TextMesh, ..Default::default() });
                V::Speedometer(self.v45.speedometers.len() - 1)
            }
            "CrossbowBehaviour" => {
                let launcher_at = field_object(prefab, o, v, "Launcher").unwrap_or_else(|| unimplemented!("mod-only: CrossbowBehaviour.Launcher null"));
                let Launcher = *linked.launchers.get(&launcher_at).unwrap_or_else(|| unimplemented!("mod-only: CrossbowBehaviour.Launcher without a launcher"));
                if field_object(prefab, o, v, "AttachedSyringe").is_some() {
                    unimplemented!("mod-only: CrossbowBehaviour with a serialized AttachedSyringe");
                }
                let physical_index = prefab.objects[o].components.iter().position(|(_, k)| matches!(k, Component::Script { class, .. } if class == "PhysicalBehaviour"));
                self.v45.crossbows.push(Crossbow {
                    id,
                    enabled,
                    Launcher,
                    phys: self.launchers[Launcher].physical,
                    LocalSyringeAttachPoint: vec2(v, "LocalSyringeAttachPoint"),
                    SyringeEjectForce: f(v, "SyringeEjectForce", 1.0),
                    SyringeFluidUsage: f(v, "SyringeFluidUsage", 0.14),
                    AttachedSyringe: None,
                    syringeJoint: None,
                    lastDisconnectTime: f32::NEG_INFINITY,
                    hasSyringe: false,
                    index: c,
                    after_physical: physical_index.is_some_and(|k| c > k),
                    imbue_playing: false,
                    imbue_color: [1.0; 4],
                    imbue_emission: true,
                });
                // CrossbowBehaviour.Awake: Launcher.OnLaunch += OnLaunch (launcher.rs calls crossbow_on_launch).
                V::Crossbow(self.v45.crossbows.len() - 1)
            }
            "BuzzerBehaviour" => {
                // BuzzerBehaviour.Awake: PhysicalBehaviour and SpriteRenderer default to this object's.
                let p = physical_at(field_object(prefab, o, v, "PhysicalBehaviour")).or_else(|| linked.physicals.get(&o).copied()).unwrap_or_else(|| unimplemented!("mod-only: BuzzerBehaviour without a PhysicalBehaviour"));
                let damagable = field_object(prefab, o, v, "DamagableBehaviour").and_then(|k| linked.machinery.get(&k).copied());
                let SpriteRenderer = Some(self.v45_sprite_at(inst, linked, prefab, field_object(prefab, o, v, "SpriteRenderer").unwrap_or(o)));
                let Diaphragm = field_object(prefab, o, v, "Diaphragm").map(|k| self.v45_sprite_at(inst, linked, prefab, k));
                self.v45.buzzers.push(Buzzer {
                    id,
                    enabled,
                    PhysicalBehaviour: p,
                    DamagableBehaviour: damagable,
                    AudioData: v.get("AudioData").and_then(|s| if let Value::Str(s) = s { Some(s.clone()) } else { None }).unwrap_or_default(),
                    Loudness: f(v, "Loudness", 1.0),
                    lastPlayedTime: f32::NEG_INFINITY,
                    SpriteRenderer,
                    Diaphragm,
                    ..Default::default()
                });
                V::Buzzer(self.v45.buzzers.len() - 1)
            }
            "DrippingBloodParticleController" => {
                self.v45.drips.push(DrippingBlood {
                    id,
                    PhysicalBehaviour: physical_at(field_object(prefab, o, v, "PhysicalBehaviour")),
                    Colour: [0.0; 4],
                    Amount: f(v, "Amount", 1.0),
                    DecaySpeed: f(v, "DecaySpeed", 0.25),
                    LocalDripAreas: CircularBuffer::new(8),
                    clock: super::misc::FixedIntervalDistributor::with_rate(25.0),
                    emitted: Vec::new(),
                });
                V::DrippingBlood(self.v45.drips.len() - 1)
            }
            "MagneticBehaviour" => {
                // MagneticBehaviour.Awake: rb = GetComponent<Rigidbody2D>().
                let phys = linked.physicals.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: MagneticBehaviour without a PhysicalBehaviour's Rigidbody2D"));
                self.v45.magnets.push(Magnetic {
                    id,
                    phys,
                    EffectiveDistance: f(v, "EffectiveDistance", 1.0),
                    NorthPole: vec2(v, "NorthPole"),
                    SouthPole: vec2(v, "SouthPole"),
                    MagnetStrength: f(v, "MagnetStrength", 1.0),
                    ..Default::default()
                });
                V::Magnetic(self.v45.magnets.len() - 1)
            }
            _ => unreachable!(),
        };
        self.script_slots.insert(id, super::save_slots::Slot::V45(b));
        Some(Behaviour::V45(b))
    }

    /// The SpriteRenderer on prefab object `t` of the clone being linked (link.rs's sprite_at without its Linker).
    pub(crate) fn v45_sprite_at(&self, inst: &super::spawn::Instance, linked: &super::link::Linked, prefab: &Prefab, t: usize) -> super::visual::SpriteAt {
        use super::visual::SpriteAt;
        let k = prefab.objects[t].components.iter().position(|(_, y)| matches!(y, Component::SpriteRenderer(_))).unwrap_or_else(|| unimplemented!("mod-only: {}: no SpriteRenderer", prefab.objects[t].name));
        if let Some(&p) = linked.physicals.get(&t) {
            return SpriteAt::Physical(p);
        }
        let rid = inst.objects[t].components[k];
        SpriteAt::Renderer(self.renderers.iter().position(|r| r.id == rid).unwrap_or_else(|| unimplemented!("mod-only: {}: a SpriteRenderer without a sprite", prefab.objects[t].name)))
    }

    /// OnWillRenderObject of the new classes whose object's own sprite the camera draws (an enabled behaviour, as the
    /// generators' and displays').
    pub fn v45_will_render(&mut self) {
        use super::visual::SpriteAt;
        for s in 0..self.v45.speedometers.len() {
            let x = &self.v45.speedometers[s];
            if self.will_render_dispatch(x.id, x.enabled && !self.destroyed_components.contains(&x.id) && self.sprite_drawn(SpriteAt::Physical(x.Phys))) {
                self.speedometer_set_text(s);
            }
        }
        for b in 0..self.v45.buzzers.len() {
            let x = &self.v45.buzzers[b];
            let own = x.SpriteRenderer.unwrap_or(SpriteAt::Physical(x.PhysicalBehaviour));
            if self.will_render_dispatch(x.id, x.enabled && !self.destroyed_components.contains(&x.id) && self.sprite_drawn(own)) {
                self.buzzer_on_will_render_object(b);
            }
        }
    }

    /// The Start a new class's component queues at link (None: it declares none).
    pub(crate) fn v45_pending_start(&self, id: i32) -> Option<super::misc::PendingStart> {
        match self.behaviour_of.get(&id) {
            Some(&(_, Behaviour::V45(v @ (V::Speedometer(_) | V::Buzzer(_))))) => Some(super::misc::PendingStart::V45(v)),
            _ => None,
        }
    }

    pub(crate) fn v45_start(&mut self, v: V) {
        match v {
            V::Speedometer(s) => self.speedometer_start(s),
            V::Buzzer(b) => self.buzzer_start(b),
            V::Dulcamarus(d) => self.dulcamarus_start(d),
            V::Injectable(i) => self.injectable_start(i),
            _ => {}
        }
    }

    /// The managers' callbacks.
    pub(crate) fn v45_phase(&mut self, phase: super::Phase, v: V) {
        use super::Phase;
        match (phase, v) {
            (Phase::Fixed, V::Speedometer(s)) => self.speedometer_fixed_update(s),
            (Phase::Update, V::Crossbow(c)) => self.crossbow_update(c),
            (Phase::Update, V::Dulcamarus(d)) => self.dulcamarus_update(d),
            (Phase::Update, V::DrippingBlood(d)) => self.dripping_blood_update(d),
            (Phase::Fixed, V::Magnetic(m)) => self.magnetic_fixed_update(m),
            (phase, v) => unreachable!("{v:?} in the {phase:?} manager"),
        }
    }

    /// Messages.IUse.Use
    pub(crate) fn v45_use(&mut self, v: V) {
        match v {
            V::Speedometer(s) => self.speedometer_use(s),
            V::Buzzer(b) => self.buzzer_use(b),
            _ => unreachable!(),
        }
    }

    // ---- MagneticBehaviour ---------------------------------------------------------------------------------------

    /// MagneticBehaviour.FixedUpdate
    pub(crate) fn magnetic_fixed_update(&mut self, m: usize) {
        use super::shatter::ShatterHost;
        let p = self.v45.magnets[m].phys;
        let rb = self.physicals[p].rigidbody.clone();
        let (north, south) = (self.v45.magnets[m].NorthPole, self.v45.magnets[m].SouthPole);
        self.v45.magnets[m].globalNorthPole = self.rigidbody_point_relative(&rb, north);
        self.v45.magnets[m].globalSouthPole = self.rigidbody_point_relative(&rb, south);
        let center = <Game as ShatterHost>::world_center_of_mass(self, p);
        let (mut val, mut val2) = ([0.0f32; 2], [0.0f32; 2]);
        // Physics2D.OverlapCircleNonAlloc(worldCenterOfMass, EffectiveDistance, buffer): 16 colliders at most.
        let hits = self.overlap_circle(crate::b2::math::Vec2::new(center[0], center[1]), self.v45.magnets[m].EffectiveDistance, super::explosion::DEFAULT_RAYCAST_LAYERS);
        for (q, collider) in hits.into_iter().take(16) {
            // PhysicalObjectsInWorldByTransform.TryGetValue(collider.transform): the collider sits on the physical's own object.
            let Some(q) = q.filter(|&q| self.physicals[q].in_world && !self.physicals[q].destroyed && Some(self.physical_game_object(q)) == self.world.collider_game_object(collider)) else { continue };
            if !(mathf::clamp01(self.properties[self.physicals[q].Properties].MagneticAttractionIntensity) > mathf::EPSILON) {
                continue;
            }
            let a = self.magnetic_attract(m, q, Pole::North);
            val = [val[0] + a[0], val[1] + a[1]];
            let b = self.magnetic_attract(m, q, Pole::South);
            val2 = [val2[0] + b[0], val2[1] + b[1]];
        }
        let (gn, gs) = (self.v45.magnets[m].globalNorthPole, self.v45.magnets[m].globalSouthPole);
        // sqrMagnitude in double (Mono): a force too small to square in float still pushes.
        if mathf::sqr_magnitude(val) > 0.0 {
            rb.add_force_at_position(&mut self.world, val, gn);
        }
        if mathf::sqr_magnitude(val2) > 0.0 {
            rb.add_force_at_position(&mut self.world, val2, gs);
        }
    }

    /// MagneticBehaviour.GetGlobalPole
    pub(crate) fn magnetic_global_pole(&self, m: usize, pole: Pole) -> [f32; 2] {
        match pole {
            Pole::North => self.v45.magnets[m].globalNorthPole,
            Pole::South => self.v45.magnets[m].globalSouthPole,
        }
    }

    /// MagneticBehaviour.Attract: another magnet pulls on both its poles; anything else is pulled at its closest point.
    pub(crate) fn magnetic_attract(&mut self, m: usize, other: usize, pole: Pole) -> [f32; 2] {
        let global = self.magnetic_global_pole(m, pole);
        // other.TryGetComponent<MagneticBehaviour>: the first on its object.
        let destroyed = &self.destroyed_components;
        if let Some(k) = self.v45.magnets.iter().position(|x| x.phys == other && !destroyed.contains(&x.id)) {
            let strength = self.v45.magnets[m].MagnetStrength;
            let n = magnetic_get_force(strength, self.magnetic_global_pole(k, Pole::North), global);
            let s = magnetic_get_force(strength, self.magnetic_global_pole(k, Pole::South), global);
            let (sn, ss) = (magnetic_get_pole_sign(pole, Pole::North), magnetic_get_pole_sign(pole, Pole::South));
            let (val, val2) = ([n[0] * sn, n[1] * sn], [s[0] * ss, s[1] * ss]);
            return [-(val[0] + val2[0]), -(val[1] + val2[1])];
        }
        // other.rigidbody.ClosestPoint(globalPole)
        let val3 = self.rigidbody_closest_point(other, global);
        let force = magnetic_get_force(self.v45.magnets[m].MagnetStrength, val3, global);
        let rb = self.physicals[other].rigidbody.clone();
        rb.add_force_at_position(&mut self.world, force, val3);
        [-force[0], -force[1]]
    }

    // ---- SpeedometerBehaviour ------------------------------------------------------------------------------------

    /// SpeedometerBehaviour.Start: UpdateActivation, then the "resetTopSpeed" button (shown while TopSpeedMs > 0).
    pub(crate) fn speedometer_start(&mut self, s: usize) {
        self.speedometer_update_activation(s);
        let p = self.v45.speedometers[s].Phys;
        let label = crate::il45!(str "SpeedometerBehaviour::Start" 1);
        let button = super::menu::MenuButton { condition: super::menu::Cond::SpeedometerRecord(s), ..super::menu::MenuButton::new("resetTopSpeed", label, crate::il45!(str "SpeedometerBehaviour::Start" 2), super::menu::MenuAction::SpeedometerReset(s)) };
        self.physicals[p].menu.push(button);
    }

    /// SpeedometerBehaviour.Use
    pub(crate) fn speedometer_use(&mut self, s: usize) {
        if self.v45.speedometers[s].enabled {
            self.v45.speedometers[s].Activated = !self.v45.speedometers[s].Activated;
            self.speedometer_update_activation(s);
        }
    }

    /// SpeedometerBehaviour.UpdateActivation (also SpeedometerBehaviour.OnEnable): TextMesh.enabled, then Light.enabled = Activated.
    pub(crate) fn speedometer_update_activation(&mut self, s: usize) {
        let on = self.v45.speedometers[s].Activated;
        self.speedometer_show(s, on);
    }

    /// SpeedometerBehaviour.OnDisable: presentation (compared with real 1.28.45: reference-probe/game/v45vis_1.txt): TextMesh.enabled, then Light.enabled = false.
    pub(crate) fn speedometer_on_disable(&mut self, s: usize) {
        self.speedometer_show(s, false);
    }

    /// `Light.enabled = TextMesh.enabled = on` (a null one throws, the TextMesh first).
    pub(crate) fn speedometer_show(&mut self, s: usize, on: bool) {
        let (light, text) = (self.v45.speedometers[s].Light, self.v45.speedometers[s].TextMesh);
        self.text_enabled_set(Self::text_ref(text), on);
        let light = light.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.set_sprite_enabled(light, on);
    }

    /// SpeedometerBehaviour.FixedUpdate
    pub(crate) fn speedometer_fixed_update(&mut self, s: usize) {
        let p = self.v45.speedometers[s].Phys;
        let v = self.physicals[p].rigidbody.velocity(&self.world);
        let x = &mut self.v45.speedometers[s];
        x.speedMs = mathf::magnitude2(v) * 0.82397;
        x.TopSpeedMs = mathf::max(x.TopSpeedMs, x.speedMs);
    }

    // SpeedometerBehaviour.OnWillRenderObject: presentation (compared with real 1.28.45: reference-probe/game/v45vis_1.txt): SetText (v45_will_render).

    /// SpeedometerBehaviour.SetText: the TextMesh's text, both speeds in the SpeedUnit preference.
    pub fn speedometer_set_text(&mut self, s: usize) {
        // UserPreferenceManager.Current.SpeedUnit
        let unit = SpeedUnit::from_index(self.prefs.SpeedUnit);
        let x = &self.v45.speedometers[s];
        let tag = crate::il45!(str "SpeedometerBehaviour::SetText" 0);
        let text = format!("{}{tag}{}", convert_speed(x.speedMs, unit), convert_speed(x.TopSpeedMs, unit));
        self.text_write(x.TextMesh, &text);
    }

    // ---- CrossbowBehaviour and InjectableBehaviour ---------------------------------------------------------------

    /// The crossbows that take a physical's collision messages: (component index, CollisionScript).
    pub(crate) fn v45_collision_scripts(&self, p: usize, after: bool) -> Vec<(usize, super::acts::CollisionScript)> {
        if !crate::assets::il::is_1_28_45() {
            return Vec::new();
        }
        (0..self.v45.crossbows.len()).filter(|&c| self.v45.crossbows[c].phys == p && self.v45.crossbows[c].after_physical == after).map(|c| (self.v45.crossbows[c].index, super::acts::CollisionScript::Crossbow(c))).collect()
    }

    /// CrossbowBehaviour.OnCollisionEnter2D
    pub(crate) fn crossbow_on_collision_enter(&mut self, c: usize, col: &crate::b2::contacts2d::ScriptCollision) {
        let x = &self.v45.crossbows[c];
        if x.hasSyringe || self.crossbow_joint_alive(c) || self.crossbow_attached(c).is_some() || self.time.time - x.lastDisconnectTime < 1.0 {
            return;
        }
        let Some(other) = self.collision_physical(col) else { return };
        let at = self.physicals[other].transform.position;
        let local = self.inverse_transform_point(x.phys, at);
        let delta = [local[0] - x.LocalSyringeAttachPoint[0], local[1] - x.LocalSyringeAttachPoint[1]];
        if !(mathf::sqr_magnitude(delta) < 0.040000003) {
            return;
        }
        let Some(s) = self.syringes.iter().position(|s| s.Phys == other) else { return };
        if !self.syringes[s].IsAttachedToLauncher && self.syringes[s].pushTargets.iter().flatten().count() == 0 {
            self.crossbow_connect_syringe(c, s);
        }
    }

    /// CrossbowBehaviour.ConnectSyringe
    pub(crate) fn crossbow_connect_syringe(&mut self, c: usize, s: usize) {
        let me = self.v45.crossbows[c].phys;
        let sp = self.syringes[s].Phys;
        let attach = self.v45.crossbows[c].LocalSyringeAttachPoint;
        let at = self.transform_point(me, attach);
        self.physical_set_position(sp, [at[0], at[1], self.physicals[sp].transform.position[2]]);
        self.physical_set_rotation(sp, self.physicals[me].transform.rotation);
        let (ss, ms) = (self.physicals[sp].transform.localScale, self.physicals[me].transform.localScale);
        if (ss[0] > 0.0) != (ms[0] > 0.0) {
            self.physical_set_local_scale(sp, [ss[0] * -1.0, ss[1], ss[2]]);
        }
        self.v45.crossbows[c].AttachedSyringe = Some(s);
        self.syringes[s].IsAttachedToLauncher = true;
        // syringe.PressureMode = PressureDirection.None
        self.syringes[s].PressureMode = 2;
        // AddComponent<FixedJoint2D>: its instance id.
        self.allocate_instance_id();
        let j = self.add_fixed_joint_component(me);
        // A second ConnectSyringe (OnAfterDeserialise with the syringe connected) leaves the old joint in place, no longer syringeJoint.
        if let Some(old) = self.v45.crossbows[c].syringeJoint.filter(|&o| !self.fixed_joints[o].destroyed) {
            self.loose_fixed_joints.push(old);
        }
        self.fixed_joint_set_connected_body(j, Some(super::BodyOwner::Physical(sp)));
        self.fixed_joint_set_anchor(j, attach);
        self.fixed_joint_set_connected_anchor(j, [0.0, 0.0]);
        self.fixed_joint_set_enable_collision(j, false);
        self.v45.crossbows[c].syringeJoint = Some(j);
        let clip = self.audio.clip(self.v45.crossbows[c].id, "SyringeConnectClip");
        self.play_clip_once(me, clip, 1.0);
        // ImbueEffect.main.startColor = syringe.GetComputedColor(); ImbueEffect.Play()
        let o = Owner::Syringe(s);
        let fallback = self.bc(o).cachedComputedColour;
        let color = self.get_computed_color(o, fallback);
        let x = &mut self.v45.crossbows[c];
        x.imbue_color = color;
        x.imbue_playing = true;
        if let Some(i) = self.field_ps(self.v45.crossbows[c].id, "ImbueEffect") {
            self.ps_calls.push((i, super::PsCall::Play));
        }
        // syringe.gameObject.SetLayer(10)
        self.v45_set_layer(sp, 10);
        // component.PauseDragOverride = false; component.PreventGripping = true
        self.physicals[sp].PauseDragOverride = false;
        self.physicals[sp].PreventGripping = true;
        self.v45.crossbows[c].hasSyringe = true;
    }

    /// Utils.SetLayer(gameObject, layer) on a physical's object (children tagged RecursiveLayerChild: none on a syringe).
    pub(crate) fn v45_set_layer(&mut self, p: usize, layer: i32) {
        let id = self.physicals[p].id;
        self.with_component_instance(id, |g, inst, o| g.game_object_set_layer(inst, o, layer));
    }

    /// `(bool)syringeJoint`: the FixedJoint2D ConnectSyringe added, not yet destroyed.
    pub(crate) fn crossbow_joint_alive(&self, c: usize) -> bool {
        self.v45.crossbows[c].syringeJoint.is_some_and(|j| !self.fixed_joints[j].destroyed)
    }

    /// `(bool)AttachedSyringe`: the syringe it holds, while its object lives.
    pub(crate) fn crossbow_attached(&self, c: usize) -> Option<usize> {
        self.v45.crossbows[c].AttachedSyringe.filter(|&s| !self.physicals[self.syringes[s].Phys].destroyed)
    }

    /// CrossbowBehaviour.DisconnectSyringe
    pub(crate) fn crossbow_disconnect_syringe(&mut self, c: usize) {
        if self.crossbow_joint_alive(c) {
            // Object.Destroy(syringeJoint): queued to the end of the frame.
            let j = self.v45.crossbows[c].syringeJoint.unwrap();
            if !self.joint_destroys.iter().any(|&(_, k)| k == j) {
                self.destroy_joint_delayed(j);
            }
        }
        if let Some(s) = self.crossbow_attached(c) {
            let sp = self.syringes[s].Phys;
            // StartCoroutine(Utils.DelayCoroutine(1f, () => s.SetLayer(9))): owned by the crossbow.
            let owner = self.v45.crossbows[c].id;
            self.start_wait(owner, 1.0, super::destroyable::Step::CrossbowSetLayer(sp));
            {
                // TryGetComponent<PhysicalBehaviour>: PreventGripping = false; PauseDragOverride = true.
                self.physicals[sp].PreventGripping = false;
                self.physicals[sp].PauseDragOverride = true;
                let me = self.v45.crossbows[c].phys;
                let q = self.physicals[me].transform.rotation;
                let up = mathf::rotate(q, [0.0, 1.0, 0.0]);
                let right = mathf::rotate(q, [1.0, 0.0, 0.0]);
                let dir = mathf::normalized2([up[0] - right[0] * 0.5, up[1] - right[1] * 0.5]);
                let k = self.v45.crossbows[c].SyringeEjectForce;
                let rb = self.physicals[sp].rigidbody.clone();
                rb.add_force_impulse(&mut self.world, [dir[0] * k, dir[1] * k]);
                let sign = self.physicals[me].transform.localScale[0].signum();
                rb.add_torque_impulse(&mut self.world, sign * k * 0.1);
            }
            self.v45.crossbows[c].AttachedSyringe = None;
        }
        let me = self.v45.crossbows[c].phys;
        let clip = self.audio.clip(self.v45.crossbows[c].id, "SyringeDisconnectClip");
        self.play_clip_once(me, clip, 1.0);
        let x = &mut self.v45.crossbows[c];
        x.imbue_playing = false;
        x.lastDisconnectTime = self.time.time;
        x.hasSyringe = false;
        if let Some(i) = self.field_ps(self.v45.crossbows[c].id, "ImbueEffect") {
            self.ps_calls.push((i, super::PsCall::Stop(self.time.timeAsDouble)));
        }
    }

    /// CrossbowBehaviour.Update
    pub(crate) fn crossbow_update(&mut self, c: usize) {
        if let Some(s) = self.crossbow_attached(c) {
            let o = Owner::Syringe(s);
            let fallback = self.bc(o).cachedComputedColour;
            let mut color = self.get_computed_color(o, fallback);
            color[3] = 0.02;
            let emission = self.scaled_liquid_amount(o) > 0.01;
            let x = &mut self.v45.crossbows[c];
            x.imbue_color = color;
            x.imbue_emission = emission;
            if !self.syringes[s].IsAttachedToLauncher {
                self.crossbow_disconnect_syringe(c);
            }
        } else if self.v45.crossbows[c].hasSyringe {
            self.crossbow_disconnect_syringe(c);
        }
    }

    /// The lambda of DisconnectSyringe's Utils.DelayCoroutine: s.SetLayer(9). A syringe destroyed meanwhile makes it throw
    /// (MissingReferenceException, logged by the coroutine).
    pub(crate) fn crossbow_set_layer_resume(&mut self, p: usize) {
        if self.physicals[p].destroyed {
            let msg = "MissingReferenceException: The object of type 'GameObject' has been destroyed but you are still trying to access it.\nYour script should either check if it is null or you should not destroy the object.";
            self.log_exception(Some(super::menu::ManagedException(msg.into())));
            return;
        }
        self.v45_set_layer(p, 9);
    }

    /// CrossbowBehaviour.OnJointBreak2D (sent to the broken joint's GameObject): its own syringeJoint disconnects.
    pub(crate) fn crossbow_on_joint_break(&mut self, f: usize) {
        if !crate::assets::il::is_1_28_45() {
            return;
        }
        for c in 0..self.v45.crossbows.len() {
            if self.v45.crossbows[c].syringeJoint == Some(f) && !self.destroyed_components.contains(&self.v45.crossbows[c].id) {
                self.crossbow_disconnect_syringe(c);
            }
        }
    }

    /// CrossbowBehaviour.OnAfterDeserialise: a loaded AttachedSyringe is connected again.
    pub(crate) fn crossbow_on_after_deserialise(&mut self, c: usize) {
        if let Some(s) = self.crossbow_attached(c) {
            self.crossbow_connect_syringe(c, s);
        }
    }

    /// CrossbowBehaviour.OnPreFlip: an attached syringe that is not selected is parented to the crossbow for the flip, and
    /// Utils.NextFrameCoroutine sets its parent back to null at the end of the frame.
    pub(crate) fn crossbow_on_pre_flip(&mut self, c: usize) {
        let Some(s) = self.crossbow_attached(c) else { return };
        let sp = self.syringes[s].Phys;
        if self.tool.as_ref().is_some_and(|t| t.rawSelection.contains(&sp)) {
            return;
        }
        self.crossbow_parent_syringe(c, sp);
        self.coroutine_seq += 1;
        let owner = self.v45.crossbows[c].id;
        self.end_of_frame.push(super::destroyable::Coroutine { seq: self.coroutine_seq, at: self.time.timeAsDouble, owner, from: i64::MIN, step: super::destroyable::Step::CrossbowUnparent(c) });
    }

    /// AttachedSyringe.transform.SetParent(transform) (worldPositionStays).
    pub(crate) fn crossbow_parent_syringe(&mut self, c: usize, sp: usize) {
        let me = self.v45.crossbows[c].phys;
        if self.physicals[sp].parent.is_some() {
            // From one parent to another: the world pose stays either way.
            self.physical_set_parent_null(sp);
        }
        self.physical_set_parent(sp, me);
    }

    /// The lambda of OnPreFlip's NextFrameCoroutine (`from_coroutine`), and OnPlayerDragEnd: AttachedSyringe.transform.SetParent(null).
    /// With no syringe any more the lambda throws (NullReferenceException, logged by the coroutine).
    pub(crate) fn crossbow_unparent(&mut self, c: usize, from_coroutine: bool) {
        match self.v45.crossbows[c].AttachedSyringe {
            Some(s) if !self.physicals[self.syringes[s].Phys].destroyed => self.physical_set_parent_null(self.syringes[s].Phys),
            _ if from_coroutine => self.log_exception(Some(super::menu::ManagedException("NullReferenceException: Object reference not set to an instance of an object".into()))),
            _ => {}
        }
    }

    /// CrossbowBehaviour.OnPlayerDragStart / CrossbowBehaviour.OnPlayerDragEnd (`end`) of the crossbows on `item` and below it:
    /// the attached syringe is parented to the crossbow while it is dragged in pause mode.
    pub(crate) fn crossbow_on_drag(&mut self, item: usize, end: bool) {
        if !crate::assets::il::is_1_28_45() {
            return;
        }
        for c in 0..self.v45.crossbows.len() {
            let me = self.v45.crossbows[c].phys;
            if self.destroyed_components.contains(&self.v45.crossbows[c].id) || self.physicals[me].destroyed || !self.physicals[me].active || !self.physical_is_child_of(me, item) {
                continue;
            }
            match (end, self.crossbow_attached(c)) {
                (false, Some(s)) => {
                    let sp = self.syringes[s].Phys;
                    self.crossbow_parent_syringe(c, sp);
                }
                (true, Some(_)) => self.crossbow_unparent(c, false),
                _ => {}
            }
        }
    }

    /// CrossbowBehaviour.OnLaunch (Launcher.OnLaunch): the launched bolt gets an InjectableBehaviour holding
    /// SyringeFluidUsage of each liquid the attached syringe holds.
    pub(crate) fn crossbow_on_launch(&mut self, l: usize, bolt: Option<usize>) {
        if !crate::assets::il::is_1_28_45() {
            return;
        }
        for c in 0..self.v45.crossbows.len() {
            // CrossbowBehaviour.OnDestroy: Launcher.OnLaunch -= OnLaunch, so a destroyed crossbow no longer hears it.
            if self.v45.crossbows[c].Launcher != l || self.destroyed_components.contains(&self.v45.crossbows[c].id) {
                continue;
            }
            let Some(s) = self.crossbow_attached(c) else { continue };
            let o = Owner::Syringe(s);
            if !(self.scaled_liquid_amount(o) > 0.0) {
                continue;
            }
            let fallback = self.bc(o).cachedComputedColour;
            self.get_computed_color(o, fallback);
            // e.AddComponent<InjectableBehaviour>()
            let id = self.allocate_instance_id();
            let fallback = self.bc(o).cachedComputedColour;
            let mut fx = self.get_computed_color(o, fallback);
            fx[3] = 0.02;
            let usage = self.v45.crossbows[c].SyringeFluidUsage;
            let mut out = Vec::new();
            for liquid in self.bc(o).LiquidDistribution.keys() {
                let amount = self.remove_liquid(o, liquid, usage).abs();
                out.push((super::circulation::liquid_identity(liquid).to_string(), amount));
            }
            self.v45.injectables.push(Injectable { id, phys: bolt, LiquidDistributions: Some(out), FxColor: fx, destroyed: false, ImbueParticles: None });
            self.injectable_attach(self.v45.injectables.len() - 1, true);
            self.pending_starts.push(super::misc::PendingStart::V45(V::Injectable(self.v45.injectables.len() - 1)));
        }
    }

    /// InjectableBehaviour.Start: presentation (compared with real 1.28.45: reference-probe/game/v45vis_1.txt): ImbueParticles = GetComponentInChildren<ParticleSystem>() (the bolt's "imbuefx"); when there
    /// is one, main.startColor = FxColor, then Play().
    pub(crate) fn injectable_start(&mut self, i: usize) {
        let x = &self.v45.injectables[i];
        if x.phys.is_none() || x.destroyed {
            return;
        }
        let fx = x.FxColor;
        let found = self.injectable_find_particles(i);
        self.v45.injectables[i].ImbueParticles = found;
        if let Some(e) = found {
            self.ps_calls.push((e, super::PsCall::Writes(crate::particles::PsWrites { color: Some(fx), ..Default::default() })));
            self.ps_calls.push((e, super::PsCall::Play));
        }
    }

    /// Start's `gameObject.GetComponentInChildren<ParticleSystem>()` (an `embedded_ps` index).
    pub(crate) fn injectable_find_particles(&mut self, i: usize) -> Option<usize> {
        let p = self.v45.injectables[i].phys?;
        let on = self.physicals[p].id;
        // GetComponentInChildren: this object first, then its active children depth first; the first ParticleSystem's system.
        self.with_component_instance(on, |g, inst, o| {
            let mut stack = vec![o];
            while let Some(t) = stack.pop() {
                if !inst.objects[t].active {
                    continue;
                }
                let go = inst.prefab.objects[t].game_object;
                if let Some(e) = g.embedded_ps.iter().position(|e| e.go == go && inst.objects[t].components.contains(&e.owner)) {
                    return Some(e);
                }
                stack.extend((0..inst.objects.len()).rev().filter(|&c| inst.objects[c].parent == Some(t)));
            }
            None
        })
    }

    /// InjectableBehaviour.Lodged (SendMessage("Lodged") to the stabber's GameObject).
    pub(crate) fn injectable_lodged_on(&mut self, p: usize, victim: usize) {
        if !crate::assets::il::is_1_28_45() {
            return;
        }
        for i in 0..self.v45.injectables.len() {
            let x = &self.v45.injectables[i];
            if x.destroyed || x.phys != Some(p) {
                continue;
            }
            if self.physicals[victim].destroyed || x.LiquidDistributions.as_ref().is_none_or(Vec::is_empty) {
                continue;
            }
            let Some(target) = self.physical_container(victim) else { continue };
            // Liquid.TryGetLiquid(LiquidID): an unknown id (a mod's liquid) is skipped.
            for (liquid, amount) in self.v45.injectables[i].LiquidDistributions.take().unwrap() {
                if let Some(liquid) = super::circulation::liquid_get(&liquid) {
                    self.add_liquid(target, liquid, amount);
                }
            }
            if let Some(e) = self.v45.injectables[i].ImbueParticles {
                self.ps_calls.push((e, super::PsCall::Stop(self.time.timeAsDouble)));
            }
            // Object.Destroy(this): queued (end of frame) like the others; the entry counts as spent at once.
            self.v45.injectables[i].destroyed = true;
            let (id, at) = (self.v45.injectables[i].id, self.time.fixedTimeAsDouble.min(self.time.timeAsDouble));
            if !self.component_destroys.iter().any(|&(_, c)| c == id) {
                self.component_destroys.push((at, id));
            }
        }
    }

    /// e.AddComponent<InjectableBehaviour>() on the bolt's object (`enabled`: a loaded one's Enabled), so a save packs it.
    pub(crate) fn injectable_attach(&mut self, i: usize, enabled: bool) {
        let id = self.v45.injectables[i].id;
        if let Some(p) = self.v45.injectables[i].phys {
            let on = self.physicals[p].id;
            self.attach_script(on, "InjectableBehaviour", id, enabled);
        }
        self.script_slots.insert(id, super::save_slots::Slot::V45(V::Injectable(i)));
    }

    /// The stored members of a new class's component (MonoBehaviourPrototype's storedFields; save_members_45).
    pub(crate) fn v45_extras(&mut self, v: V) -> Vec<(&'static str, super::save_extras::Extra<'_>)> {
        use super::save_extras::Extra;
        match v {
            V::Speedometer(i) => {
                let x = &mut self.v45.speedometers[i];
                vec![("Activated", Extra::F(&mut x.Activated)), ("TopSpeedMs", Extra::F(&mut x.TopSpeedMs))]
            }
            V::Crossbow(i) => {
                let x = &mut self.v45.crossbows[i];
                vec![("SyringeEjectForce", Extra::F(&mut x.SyringeEjectForce)), ("SyringeFluidUsage", Extra::F(&mut x.SyringeFluidUsage))]
            }
            V::Buzzer(i) => {
                let x = &mut self.v45.buzzers[i];
                vec![("AudioData", Extra::F(&mut x.AudioData)), ("SerialisableAudioData", Extra::Bytes(&mut x.SerialisableAudioData)), ("Loudness", Extra::F(&mut x.Loudness))]
            }
            V::Dulcamarus(i) => {
                let x = &mut self.v45.dulcamarus[i];
                vec![("Radius", Extra::F(&mut x.Radius)), ("isNew", Extra::F(&mut x.isNew))]
            }
            V::Injectable(i) => {
                let x = &mut self.v45.injectables[i];
                vec![("LiquidDistributions", Extra::Distributions(&mut x.LiquidDistributions)), ("FxColor", Extra::F(&mut x.FxColor))]
            }
            V::Magnetic(i) => {
                let x = &mut self.v45.magnets[i];
                vec![("EffectiveDistance", Extra::F(&mut x.EffectiveDistance)), ("NorthPole", Extra::F(&mut x.NorthPole)), ("SouthPole", Extra::F(&mut x.SouthPole)), ("MagnetStrength", Extra::F(&mut x.MagnetStrength))]
            }
            // [SkipSerialisation]: never packed.
            V::DrippingBlood(_) => Vec::new(),
        }
    }

    // ---- BuzzerBehaviour -----------------------------------------------------------------------------------------

    /// BuzzerBehaviour.Start: the "chooseAudioClip" and "setLoudness" buttons; the saved audio (DataCache by CurrentDataId,
    /// else Utils.FBSF(AudioData), else SerialisableAudioData as loaded) becomes the clip; then UpdateLoudness and UpdateColour.
    pub(crate) fn buzzer_start(&mut self, b: usize) {
        use super::menu::{MenuAction, MenuButton};
        let p = self.v45.buzzers[b].PhysicalBehaviour;
        self.physicals[p].menu.push(MenuButton::new("chooseAudioClip", "Pick audio", "Pick a file from your computer", MenuAction::BuzzerChooseAudio(b)));
        self.physicals[p].menu.push(MenuButton::new("setLoudness", "Set loudness", "Set buzzer loudness", MenuAction::Entry(super::menu::Entry::BuzzerLoudness(b))));
        let id = self.v45.buzzers[b].CurrentDataId;
        if let Some(v) = self.v45.buzzer_data_cache.get(&id).filter(|_| id != 0) {
            self.v45.buzzers[b].SerialisableAudioData = Some(v.clone());
        } else if !self.v45.buzzers[b].AudioData.trim().is_empty() {
            // try { Utils.FBSF(AudioData) } catch: ModdingApi.Notify, Debug.LogError(ex), null.
            let text = self.v45.buzzers[b].AudioData.clone();
            let mut out = None;
            if let Some(ex) = super::menu::catch_managed(|| out = Some(super::utils::fbsf(&text))) {
                self.notify("Buzzer contains malformed audio data".into());
                self.log_message_received(&ex.0, super::menu::LogType::Error);
            }
            self.v45.buzzers[b].SerialisableAudioData = out;
        }
        if let Some(data) = self.v45.buzzers[b].SerialisableAudioData.clone().filter(|d| !d.is_empty()) {
            let mut made = None;
            if super::menu::catch_managed(|| made = Some(self.buzzer_clip_from_data(data))).is_some() {
                // catch (Exception): UISoundBehaviour.Main.Error(), ModdingApi.Notify, null.
                self.ui_sound(super::audio::UiSoundKind::Error);
                self.notify("Buzzer contains malformed audio data".into());
                self.v45.buzzers[b].SerialisableAudioData = None;
            }
            if let Some((clip, id)) = made {
                self.v45.buzzers[b].clip = Some(clip);
                self.v45.buzzers[b].CurrentDataId = id;
            }
        }
        self.v45.buzzers[b].SerialisableAudioData = None;
        self.buzzer_update_loudness(b);
        self.buzzer_update_colour(b);
    }

    /// BuzzerBehaviour.UpdateColour: white without data, else Color.HSVToRGB(CurrentDataId % 1000 / 1000, 1, 1).
    pub(crate) fn buzzer_update_colour(&mut self, b: usize) {
        let id = self.v45.buzzers[b].CurrentDataId;
        let color = if id == 0 { [1.0; 4] } else { super::menu::hsv_to_rgb([(id % 1000) as f32 / 1000.0, 1.0, 1.0]) };
        self.v45.buzzers[b].color = color;
        if let Some(at) = self.v45.buzzers[b].SpriteRenderer {
            self.set_sprite_color(at, color);
        }
    }

    /// BuzzerBehaviour.ClipFromData (with BuzzerBehaviour.GetId): the clip of 8 kHz unsigned 8-bit mono `bytes` (at most
    /// MaxSamples), cached by its xxHash; DataCache keeps the bytes.
    pub(crate) fn buzzer_clip_from_data(&mut self, bytes: Vec<u8>) -> (super::audio::ClipRef, u32) {
        // GetId: xxHash.CalculateHash(bytes, bytes.Length, 0)
        let id = super::contraption::xxhash(&bytes, 0);
        let cached = self.v45.buzzer_clip_cache.get(&id).cloned();
        if let Some(c) = cached {
            self.v45.buzzer_data_cache.insert(id, bytes);
            return (c, id);
        }
        let samples = super::naudio::clip_from_data_samples(&bytes);
        self.v45.buzzer_data_cache.insert(id, bytes);
        // AudioClip.Create($"buzzer_clip_{id}", num, 1, 8000, false); SetData(array, 0).
        if samples.is_empty() {
            super::menu::throw("ArgumentException: Length of created clip must be larger than 0");
        }
        let clip_id = self.allocate_instance_id();
        // The 8-bit samples are (b - 128) / 128: as 16-bit PCM exactly (b - 128) * 256.
        let data = samples.iter().map(|&x| (x * 32768.0) as i16).collect();
        let pcm = crate::assets::audio::Pcm { channels: 1, frequency: 8000, data };
        let clip = crate::assets::classes::AudioClip { name: format!("buzzer_clip_{id}"), load_type: 0, channels: 1, frequency: 8000, length: samples.len() as f32 / 8000.0, resource: Default::default(), compression_format: 0 };
        let clip = std::sync::Arc::new(super::audio::Clip { asset: ObjRef { file: usize::MAX, path_id: clip_id as i64 }, clip, pcm: Some(std::sync::Arc::new(pcm)), stream: None });
        self.v45.buzzer_clip_cache.insert(id, clip.clone());
        (clip, id)
    }

    /// BuzzerBehaviour.OnBeforeSerialise: SerialisableAudioData = the first MaxSamples of DataCache[CurrentDataId].
    pub(crate) fn buzzer_on_before_serialise(&mut self, b: usize) {
        let id = self.v45.buzzers[b].CurrentDataId;
        if let Some(v) = self.v45.buzzer_data_cache.get(&id).filter(|_| id != 0) {
            self.v45.buzzers[b].SerialisableAudioData = Some(v[..v.len().min(120000)].to_vec());
        }
    }

    /// The "chooseAudioClip" button (BuzzerBehaviour.<Start> lambda): BuzzerBehaviour.OpenFile, whose FilePicker.OpenFile makes a
    /// "FilePicker" GameObject with an Optout (its GameObject, Transform and Optout ids) running FilePicker.OpenFile's coroutine:
    /// the Windows dialog on a Task, WaitUntil it completes, then the callback. A throw in the callback ends the coroutine (logged).
    pub(crate) fn buzzer_choose_audio(&mut self, b: usize) {
        let old = self.v45.buzzers[b].SerialisableAudioData.clone();
        self.allocate_instance_id();
        self.allocate_instance_id();
        let owner = self.allocate_instance_id();
        let paths = std::sync::Arc::new(std::sync::Mutex::new(None::<Vec<String>>));
        let task = paths.clone();
        // Task.Run: FilePicker.Windows.WindowsFileSelector.SelectFile("Select file", filters, null, false).
        std::thread::spawn(move || {
            // Filter..ctor: new FilePicker.Filter("WAVE files", "wav") and ("All files", "*") only store their name and extensions.
            let got = super::naudio::file_picker_open_file(&[("WAVE files", &["wav"]), ("All files", &["*"])], false);
            *task.lock().unwrap() = Some(got);
        });
        // FilePicker.MoveNext: the OpenFile iterator, as a coroutine routine (stage 1 waits, stage 2 calls back).
        // FilePicker.System.IDisposable.Dispose: the iterator's, nothing to free (the routine is dropped when done).
        // FilePicker.System.Collections.IEnumerator.get_Current: the yielded WaitUntil (the routine's return value).
        // FilePicker.System.Collections.Generic.IEnumerator<System.Object>.get_Current: the same.
        // FilePicker.System.Collections.IEnumerator.Reset: throws NotSupportedException; Unity never calls it.
        let mut stage = 0;
        self.co_start(owner, Box::new(move |g| {
            stage += 1;
            if stage == 1 {
                let done = paths.clone();
                return super::coroutine::Yield::WaitUntil(Box::new(move |_| done.lock().unwrap().is_some()));
            }
            let got = paths.lock().unwrap().take().unwrap_or_default();
            g.buzzer_open_file_callback(b, &got, old.clone());
            super::coroutine::Yield::Done
        }));
    }

    /// BuzzerBehaviour.OpenFile's FilePicker callback: one picked path is converted (BuzzerBehaviour.OpenFile's NAudio chain, naudio.rs),
    /// ClipFromData, UpdateColour, then the button's callback.
    pub(crate) fn buzzer_open_file_callback(&mut self, b: usize, paths: &[String], old: Option<Vec<u8>>) {
        if paths.len() != 1 {
            return;
        }
        let bytes = match super::naudio::buzzer_open_file_convert(std::path::Path::new(&paths[0])) {
            Ok(bytes) => bytes,
            // DialogBoxManager.Notification (too big, too long), then throw new PpgPlayerException (or the .NET exception).
            Err(e) => {
                if let Some(text) = e.notification() {
                    self.contraption.dialogs.push(super::contraption::DialogRequest::notification(text.into()));
                }
                super::menu::throw(e.exception_text())
            }
        };
        let (clip, id) = self.buzzer_clip_from_data(bytes);
        self.v45.buzzers[b].clip = Some(clip.clone());
        self.v45.buzzers[b].CurrentDataId = id;
        self.buzzer_update_colour(b);
        self.buzzer_choose_audio_done(b, Some(clip), old);
    }

    /// The button's OpenFile callback: a clip plays once unless the buzzer is destroyed; none restores the old data.
    pub(crate) fn buzzer_choose_audio_done(&mut self, b: usize, c: Option<super::audio::ClipRef>, old: Option<Vec<u8>>) {
        let Some(c) = c else {
            self.ui_sound(super::audio::UiSoundKind::Warning);
            self.notify("No file selected".into());
            self.v45.buzzers[b].SerialisableAudioData = old;
            return;
        };
        self.v45.buzzers[b].clip = Some(c.clone());
        let destroyed = self.v45.buzzers[b].DamagableBehaviour.map_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"), |m| self.machinery[m].Destroyed);
        if !destroyed {
            if let Some(source) = self.audio.source_field(self.v45.buzzers[b].id, "Source") {
                self.audio.play_one_shot(source, Some(&c), 1.0);
            }
        }
    }

    /// BuzzerBehaviour.UpdateLoudness
    pub(crate) fn buzzer_update_loudness(&mut self, b: usize) {
        let (num, num2) = (1.2136364f32, 24.272728f32);
        let x = &mut self.v45.buzzers[b];
        let num3 = mathf::clamp01(mathf::map_range(num, num2, 0.0, 1.0, x.Loudness));
        x.min_distance = mathf::lerp(num, num2, num3);
        x.max_distance = mathf::lerp(30.0, 100.0, num3);
        // Source.rolloffMode = Logarithmic, then its minDistance and maxDistance.
        let (id, min, max) = (x.id, x.min_distance, x.max_distance);
        let src = self.audio.source_field(id, "Source").unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        let s = self.audio.get_mut(src);
        s.rolloff_mode = 0;
        s.set_min_distance(min);
        s.set_max_distance(max);
    }

    /// BuzzerBehaviour.OnWillRenderObject: presentation (compared with real 1.28.45: reference-probe/game/v45vis_1.txt): Diaphragm.localScale is (1, 1, 1) while Source is silent, else y = 1 + the mean
    /// absolute value of Source.GetOutputData(SampleBuffer[128], 0).
    pub(crate) fn buzzer_on_will_render_object(&mut self, b: usize) {
        fn nre<T>() -> T {
            super::menu::throw("NullReferenceException: Object reference not set to an instance of an object")
        }
        let (id, diaphragm) = (self.v45.buzzers[b].id, self.v45.buzzers[b].Diaphragm);
        let src = self.audio.source_field(id, "Source").unwrap_or_else(nre);
        if !self.audio.is_playing(src) {
            self.set_sprite_local_scale(diaphragm.unwrap_or_else(nre), [1.0, 1.0, 1.0]);
            return;
        }
        // The 3D attenuation in the channels' volume, as audio_out mixes it.
        let listener = self.audio_listener();
        let s = self.audio.get(src);
        let p = self.audio_position(s);
        let d = ((p[0] - listener[0]).powi(2) + (p[1] - listener[1]).powi(2) + (p[2] - listener[2]).powi(2)).sqrt();
        let gain = 1.0 + (s.rolloff(d) - 1.0) * s.spatial_blend_at(d);
        let buffer = self.audio.get_output_data(src, 128, 0, gain);
        let mut num = 0.0f32;
        for v in buffer {
            num += v.abs();
        }
        num *= 1.0 / 128.0;
        self.set_sprite_local_scale(diaphragm.unwrap_or_else(nre), [1.0, num + 1.0, 1.0]);
    }

    /// BuzzerBehaviour.Use: Source.PlayOneShot(clip ?? FallbackAudioClip) at most every 0.1 s, unless destroyed.
    pub(crate) fn buzzer_use(&mut self, b: usize) {
        let x = &self.v45.buzzers[b];
        let destroyed = x.DamagableBehaviour.map_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"), |m| self.machinery[m].Destroyed);
        if x.enabled && !destroyed && !(self.time.time - x.lastPlayedTime < 0.1) {
            let id = x.id;
            // clip ? clip : FallbackAudioClip
            let clip = x.clip.clone().or_else(|| self.audio.clip(id, "FallbackAudioClip"));
            if let Some(source) = self.audio.source_field(id, "Source") {
                self.audio.play_one_shot(source, clip.as_ref(), 1.0);
            }
            self.v45.buzzers[b].lastPlayedTime = self.time.time;
        }
    }

    // ---- DulcamarusBehaviour -------------------------------------------------------------------------------------

    /// Whether the person's GameObject has a DulcamarusBehaviour (HasComponent).
    pub(crate) fn dulcamarus_on(&self, person: usize) -> bool {
        self.v45.dulcamarus.iter().any(|d| d.Person == person && !d.destroyed)
    }

    /// gameObject.AddComponent<DulcamarusBehaviour>() on a person: Awake now, Update from the next frame, Start pending.
    pub(crate) fn dulcamarus_add(&mut self, person: usize) -> usize {
        let id = self.allocate_instance_id();
        // DulcamarusBehaviour.Awake: head = Person.Limbs.FirstOrDefault(HasBrain); new GameObject("siphoning tendrils", typeof(Optout))
        // under this transform takes the GameObject's, Transform's and Optout's ids; Resources.Load("Materials/DivineLight").
        let head = self.people[person].Limbs.iter().copied().find(|&l| self.limbs[l].HasBrain);
        for _ in 0..3 {
            self.allocate_instance_id();
        }
        self.v45.dulcamarus.push(Dulcamarus {
            id,
            Person: person,
            Radius: 5.0,
            isNew: true,
            head,
            indexByLimb: Default::default(),
            siphonSources: Default::default(),
            shouldHeal: false,
            distributor: super::misc::FixedIntervalDistributor::with_rate(1.0),
            destroyed: false,
        });
        let d = self.v45.dulcamarus.len() - 1;
        // The component is on the person's object, so a save packs it.
        let on = self.people[person].id;
        self.attach_script(on, "DulcamarusBehaviour", id, true);
        self.script_slots.insert(id, super::save_slots::Slot::V45(V::Dulcamarus(d)));
        let b = Behaviour::V45(V::Dulcamarus(d));
        self.behaviour_of.insert(id, (0, b));
        self.behaviours.add(0, b);
        self.pending_starts.push(super::misc::PendingStart::V45(V::Dulcamarus(d)));
        d
    }

    /// DulcamarusBehaviour.Start: a DivineGlow at each connected limb, by a coin flip each.
    pub(crate) fn dulcamarus_start(&mut self, d: usize) {
        if !self.v45.dulcamarus[d].isNew {
            return;
        }
        let person = self.v45.dulcamarus[d].Person;
        for l in self.people[person].Limbs.clone() {
            if self.nodes[l].IsConnectedToRoot && self.random.value() > 0.5 {
                let at = self.physicals[self.limbs[l].PhysicalBehaviour].transform.position;
                self.spawn_resource("Prefabs/DivineGlow", at);
            }
        }
        self.v45.dulcamarus[d].isNew = false;
    }

    pub(crate) fn limb_position(&self, l: usize) -> [f32; 3] {
        self.physicals[self.limbs[l].PhysicalBehaviour].transform.position
    }

    pub(crate) fn limb_alive(&self, l: usize) -> bool {
        !self.physicals[self.limbs[l].PhysicalBehaviour].destroyed
    }

    /// DulcamarusBehaviour.Update
    pub(crate) fn dulcamarus_update(&mut self, d: usize) {
        let head = self.v45.dulcamarus[d].head.filter(|&h| self.limb_alive(h));
        if head.is_none_or(|h| self.physicals[self.limbs[h].PhysicalBehaviour].isDisintegrated || self.limbs[h].IsZombie) {
            // The tendrils are destroyed (their GameObjects).
            for s in self.v45.dulcamarus[d].siphonSources.clone().iter().flatten() {
                if let Some(t) = &s.LineRenderer {
                    self.dulcamarus_destroy_tendril(t.line);
                }
            }
            let x = &mut self.v45.dulcamarus[d];
            x.siphonSources = Default::default();
            x.indexByLimb.clear();
            x.shouldHeal = false;
        }
        let Some(head) = head else { super::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        let time = self.time.time;
        let dt = self.time.deltaTime;
        let position = self.limb_position(head);
        for i in 0..16 {
            let Some(mut s) = self.v45.dulcamarus[d].siphonSources[i].take() else { continue };
            if let Some(t) = &s.LineRenderer {
                let line = &mut self.lines[t.line];
                let seed_frac = s.Seed - s.Seed.floor();
                let num = mathf::clamp01(s.Strength) * mathf::clamp01(s.Time * mathf::lerp(1.0, 0.5, seed_frac));
                let num2 = num * num;
                line.gradient.set_start_color([num, num, num, 1.0]);
                line.gradient.set_end_color([num2, num2, num2, 1.0]);
                let num3 = mathf::map_range(0.0, 1.0, 1.0, 4.0, (s.Seed / 2.0).floor());
                let sa = |past: [f32; 3], target: [f32; 3], speed: f32| [super::fire::smooth_approach(past[0], target[0], speed, dt), super::fire::smooth_approach(past[1], target[1], speed, dt), 0.0];
                s.SourcePos = sa(s.SourcePos, position, num3 * 2.0);
                if self.limb_alive(s.Limb) {
                    s.TargetPos = sa(s.TargetPos, self.limb_position(s.Limb), num3);
                }
                let line = &mut self.lines[t.line];
                let n = line.positions.len();
                for j in 0..n {
                    let num4 = j as f32 / (n as f32 - 1.0);
                    let mut val = [0.0; 3];
                    for k in 0..3 {
                        val[k] = s.SourcePos[k] + (s.TargetPos[k] - s.SourcePos[k]) * num4;
                    }
                    let w = mathf::clamp01((num4 * std::f32::consts::PI).sin()) * 0.5;
                    let p = mathf::perlin2_mapped(val[0] + s.Seed, val[1] + time * 0.3);
                    line.positions[j] = [val[0] + w * p[0], val[1] + w * p[1], val[2]];
                }
                line.enabled = true;
            }
            s.Time += dt;
            s.Strength -= dt * 0.1;
            // DulcamarusBehaviour.IsDeadSource: Strength <= 0.
            if s.Strength <= 0.0 {
                // The LineRenderer's GameObject is destroyed; indexByLimb.Remove(Limb).
                if let Some(t) = &s.LineRenderer {
                    self.dulcamarus_destroy_tendril(t.line);
                }
                self.v45.dulcamarus[d].indexByLimb.remove(&s.Limb);
            } else {
                self.v45.dulcamarus[d].siphonSources[i] = Some(s);
            }
        }
        let person = self.v45.dulcamarus[d].Person;
        let mut k = 0;
        // The loop condition calls CalculateCycleCount on every pass.
        while k < self.v45.dulcamarus[d].distributor.calculate_cycle_count(dt) {
            k += 1;
            self.v45.dulcamarus[d].shouldHeal = false;
            let pe = &self.people[person];
            let mut flag = pe.BrainDamaged || (pe.Consciousness as f64) < 0.9 || pe.Braindead || pe.Consciousness < 0.5 || !self.person_is_alive(person);
            if !flag {
                flag = self.people[person].Limbs.iter().any(|&l| self.limbs[l].Health < self.limbs[l].InitialHealth);
            }
            if !flag {
                continue;
            }
            let num5 = dt * 0.45;
            let num6 = mathf::pow(0.9, dt);
            let radius = self.v45.dulcamarus[d].Radius;
            let nearby = self.physics_objects_near_position_accurate_non_alloc([position[0], position[1]], radius, 64);
            for q in nearby {
                if self.physicals[q].destroyed {
                    continue;
                }
                let Some(l) = self.limbs.iter().position(|x| x.PhysicalBehaviour == q) else { continue };
                let limb = &self.limbs[l];
                if limb.IsAndroid || limb.IsZombie || !(limb.Health > 0.01) || limb.Person == person || limb.ImmuneToDamage {
                    continue;
                }
                if let Some(&value) = self.v45.dulcamarus[d].indexByLimb.get(&l) {
                    let s = self.v45.dulcamarus[d].siphonSources[value].as_mut().unwrap();
                    s.Strength = 1.0;
                    if s.Time < 1.0 {
                        continue;
                    }
                    // DulcamarusBehaviour.TryGetFreeSource: the first slot that IsDeadSource (null or Strength <= 0; a dead source's slot is emptied below).
                } else if let Some(value) = self.v45.dulcamarus[d].siphonSources.iter().position(Option::is_none) {
                    self.v45.dulcamarus[d].indexByLimb.insert(l, value);
                    // SiphonSource..ctor: Seed = Random.value * 1000 (the field initialiser) draws first, then the object
                    // initialiser's LineRenderer coin flip.
                    let seed = self.random.value() * 1000.0;
                    let target = self.limb_position(l);
                    let line = if self.random.value() > 0.5 { Some(self.dulcamarus_create_line_renderer(head)) } else { None };
                    self.v45.dulcamarus[d].siphonSources[value] = Some(SiphonSource { Limb: l, SourcePos: position, TargetPos: target, Time: 0.0, Strength: 1.0, Seed: seed, LineRenderer: line });
                }
                self.v45.dulcamarus[d].shouldHeal = true;
                let color = self.limbs[l].Color;
                if grayscale(color) > 0.5 {
                    self.limbs[l].Color = [color[0] * num6, color[1] * num6, color[2] * num6, color[3]];
                }
                self.drain(Owner::Circulation(l), num5);
                self.limb_damage(l, num5);
                self.limbs[l].InternalTemperature *= num6;
                if self.skins[l].acidProgress < 0.5 {
                    let acid = self.skins[l].acidProgress + num5;
                    self.skins[l].set_acid(acid);
                }
            }
        }
        if self.v45.dulcamarus[d].shouldHeal {
            self.dulcamarus_heal(d);
        }
    }

    /// DulcamarusBehaviour.CreateLineRenderer: new GameObject("tendril") with a LineRenderer (their ids), width
    /// Random.Range(0.05, 0.4), 12 positions.
    pub(crate) fn dulcamarus_create_line_renderer(&mut self, head: usize) -> Tendril {
        self.allocate_instance_id();
        self.allocate_instance_id();
        let id = self.allocate_instance_id();
        let width = self.random.range(0.05, 0.4);
        // material = divineLightMat (Resources.Load<Material>("Materials/DivineLight")); startColor black, endColor white.
        let db = self.assets.as_ref().expect("Resources.Load without the game's data");
        let material = db.resource("Materials/DivineLight", 21).ok().flatten().expect("Resources.Load(Materials/DivineLight) found no Material");
        let mut gradient = super::visual::Gradient::default();
        gradient.set_start_color([0.0, 0.0, 0.0, 1.0]);
        gradient.set_end_color([1.0; 4]);
        let line = super::line::LineVisual {
            id,
            // Under the person (the container is a child of its transform): hidden once the head's object is gone (lines::shown).
            // DulcamarusBehaviour.OnDestroy: Destroy(lineRendererContainer): the same, as only the person's destruction removes the component.
            anchor: super::BareAnchor::Physical(self.limbs[head].PhysicalBehaviour),
            chain: Vec::new(),
            positions: vec![[0.0; 3]; 12],
            width,
            curve: crate::wires::WidthCurve::constant(1.0),
            gradient,
            material: Some(material),
            layer: 0,
            order: 0,
            game_layer: 0,
            cap_vertices: 0,
            corner_vertices: 0,
            alignment: 0,
            looped: false,
            texture_mode: 0,
            use_world_space: true,
            enabled: false,
            visible: true,
        };
        // A destroyed tendril's slot takes the new one (as trail_add's), else a new slot.
        let l = match self.v45.free_tendrils.pop() {
            Some(l) => {
                self.lines[l] = line;
                l
            }
            None => {
                self.lines.push(line);
                self.lines.len() - 1
            }
        };
        Tendril { id, line: l }
    }

    /// Object.Destroy of a tendril's GameObject: its line stops drawing and its slot is free.
    pub(crate) fn dulcamarus_destroy_tendril(&mut self, l: usize) {
        self.lines[l].visible = false;
        self.v45.free_tendrils.push(l);
    }

    /// DulcamarusBehaviour.Heal
    pub(crate) fn dulcamarus_heal(&mut self, d: usize) {
        let dt = self.time.deltaTime;
        let num = mathf::pow(0.5, dt);
        let person = self.v45.dulcamarus[d].Person;
        let pe = &mut self.people[person];
        pe.PainLevel *= num;
        pe.ShockLevel *= num;
        pe.OxygenLevel += dt;
        pe.Consciousness += dt;
        pe.Braindead = false;
        pe.BrainDamaged = false;
        pe.BrainDamagedTime = 0.0;
        pe.SeizureTime = 0.0;
        for l in self.people[person].Limbs.clone() {
            if !self.limb_alive(l) || !self.nodes[l].IsConnectedToRoot {
                continue;
            }
            let limb = &mut self.limbs[l];
            limb.Health = limb.InitialHealth;
            limb.InternalTemperature = super::fire::smooth_approach(limb.InternalTemperature, limb.BodyTemperature, 15.0, dt);
            self.limb_heal_bone(l);
            let limb = &mut self.limbs[l];
            limb.Numbness = 0.0;
            limb.LungsPunctured = false;
            limb.Frozen = false;
            let c = &mut self.circulations[l];
            c.IsPump = c.WasInitiallyPumping;
            c.InternalBleedingIntensity = 0.0;
            c.BleedingRate = 0.0;
            let original = self.blood_type(l);
            let o = Owner::Circulation(l);
            for liquid in self.bc(o).LiquidDistribution.keys() {
                if liquid != original {
                    self.remove_liquid(o, liquid, dt);
                }
            }
            if self.get_amount(o, original) < 1.0 {
                self.add_liquid(o, original, dt);
            }
            let skin = &mut self.skins[l];
            let acid = skin.acidProgress - dt;
            skin.set_acid(acid);
            let rotten = skin.rottenProgress - dt;
            skin.set_rotten(rotten);
            let p = self.limbs[l].PhysicalBehaviour;
            self.extinguish(p);
            self.physicals[p].burnIntensity *= num;
            self.physicals[p].BurnProgress -= dt;
            for point in self.skins[l].damagePoints.iter_mut() {
                point[2] *= num;
            }
        }
    }

    /// Global.GetPhysicsObjectsNearPositionAccurateNonAlloc(position, radius, buffer of `capacity`): the
    /// GetPhysicsObjectsNearPositionAccurate cells without the accuracy draws, stopping when the buffer is full.
    pub(crate) fn physics_objects_near_position_accurate_non_alloc(&mut self, position: [f32; 2], radius: f32, capacity: usize) -> Vec<usize> {
        self.refresh_physical_chunks();
        let num = radius.ceil() as i32;
        let num2 = num as f32 * 0.5;
        let mut out = Vec::new();
        for i in 0..num {
            for j in 0..num {
                let v = [position[0] - num2 + i as f32, position[1] - num2 + j as f32];
                let key = self.chunk_near([v[0], v[1], 0.0], 0.0);
                let Some(chunk) = self.physical_chunks.get(&key) else { continue };
                for &o in chunk {
                    if !self.physicals[o].isDisintegrated {
                        out.push(o);
                        if out.len() == capacity {
                            return out;
                        }
                    }
                }
            }
        }
        out
    }

    // ---- DrippingBloodParticleController -------------------------------------------------------------------------

    /// DrippingBloodParticleController.Update
    pub(crate) fn dripping_blood_update(&mut self, d: usize) {
        let Some(p) = self.v45.drips[d].PhysicalBehaviour.filter(|&p| !self.physicals[p].destroyed) else { return };
        let dt = self.time.deltaTime;
        if self.physicals[p].IsUnderWater || self.physicals[p].isDisintegrated {
            self.v45.drips[d].Amount -= dt;
            return;
        }
        let x = &mut self.v45.drips[d];
        x.Amount = f32::max(0.0, x.Amount - dt * x.DecaySpeed);
        if !(x.Amount > 0.0) {
            return;
        }
        let mut i = 0;
        while i < self.v45.drips[d].clock.calculate_cycle_count(dt) {
            i += 1;
            if self.random.value() < self.v45.drips[d].Amount {
                let areas = self.v45.drips[d].LocalDripAreas.clone();
                let a = areas.pick_random(self);
                let r = self.random.inside_unit_circle();
                let local = [a[0] + r[0] * 0.25, a[1] + r[1] * 0.25];
                // Rigidbody2D.GetRelativePoint
                self.sync_pending();
                let xf = self.world.body(self.physicals[p].rigidbody.body).xf;
                let mut at = [xf.q.c * local[0] - xf.q.s * local[1] + xf.p.x, xf.q.s * local[0] + xf.q.c * local[1] + xf.p.y];
                // SlashColliders.PickRandom().ClosestPoint(val) (Utils.PickRandom: Random.Range(0, Length)).
                if let Some(cs) = self.physicals[p].SlashColliders.clone().filter(|c| !c.is_empty()) {
                    let c = cs[self.random.range_int(0, cs.len() as i32) as usize];
                    if c == 0 {
                        super::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
                    }
                    let q = self.world.collider_closest_point(c, crate::b2::math::Vec2 { x: at[0], y: at[1] });
                    at = [q.x, q.y];
                }
                let colour = self.v45.drips[d].Colour;
                self.v45.drips[d].emitted.push((at, colour));
                // particleSystem.Emit(new EmitParams { startColor = Colour, position = val }, 1)
                let position = Some(bevy::math::Vec3::new(at[0], at[1], 0.0));
                self.ps_emit_params(|x| matches!(x.drive, super::EmbeddedDrive::Drip(e) if e == d), crate::particles::EmitParams { position, start_color: Some(colour), ..Default::default() });
            }
        }
    }

    /// PhysicalBehaviour.Start: Instantiate(Global.main.DrippingParticlesPrefab, transform, false), localPosition = 0,
    /// GetComponent<DrippingBloodParticleController>().PhysicalBehaviour = this. The clone's ids and its Update.
    pub(crate) fn dripping_blood_instantiate(&mut self, p: usize) -> usize {
        self.drip_prefab();
        let (objects, (o, k), amount, decay, system) = self.v45.drip_prefab.clone().unwrap();
        // Each object's id, then its components' (the Transform first), going down by 2 (as Global.MetalScrapePrefab's).
        let mut id = self.next_instance_id;
        let mut controller = 0;
        let nodes: Vec<(i32, Option<usize>)> = objects.iter().enumerate().map(|(j, &(n, up))| {
            if j == o {
                controller = id - 2 * (1 + k as i32);
            }
            let t = id - 2;
            id -= 2 * n as i32;
            (t, up)
        }).collect();
        self.add_runtime_clone(self.physicals[p].transform_id, &nodes);
        for _ in 0..objects.iter().map(|x| x.0).sum::<usize>() {
            self.allocate_instance_id();
        }
        self.v45.drips.push(DrippingBlood {
            id: controller,
            PhysicalBehaviour: Some(p),
            Colour: [0.0; 4],
            Amount: amount,
            DecaySpeed: decay,
            LocalDripAreas: CircularBuffer::new(8),
            clock: super::misc::FixedIntervalDistributor::with_rate(25.0),
            emitted: Vec::new(),
        });
        let d = self.v45.drips.len() - 1;
        // The clone's ParticleSystem, drawn by the embedded systems (Emit(EmitParams) in Update).
        if let Some((go, chain)) = system {
            self.embedded_ps.push(super::EmbeddedPs { go, anchor: super::BareAnchor::Physical(p), chain, owner: controller, drive: super::EmbeddedDrive::Drip(d), shape_texture: None, awake: true, emission: None });
        }
        let b = Behaviour::V45(V::DrippingBlood(d));
        self.behaviour_of.insert(controller, (0, b));
        self.behaviours.add(0, b);
        d
    }

    /// Global.main.DrippingParticlesPrefab's layout, read once.
    fn drip_prefab(&mut self) {
        if self.v45.drip_prefab.is_none() {
            let source = self.global_prefab("DrippingParticlesPrefab");
            let db = self.assets.as_mut().expect("the game's data");
            let prefab = db.prefab(source).expect("Global.DrippingParticlesPrefab");
            let mut at = None;
            let mut ps = None;
            for (o, obj) in prefab.objects.iter().enumerate() {
                for (k, (_, c)) in obj.components.iter().enumerate() {
                    match c {
                        Component::Script { class, fields, .. } if class == "DrippingBloodParticleController" => {
                            at = Some((o, k, f(fields, "Amount", 1.0), f(fields, "DecaySpeed", 0.25)));
                            ps = field_object(&prefab, o, fields, "ParticleSystem");
                        }
                        // Optout is a marker (no messages).
                        Component::Script { class, .. } if class == "Optout" => {}
                        Component::Script { class, .. } => unimplemented!("mod-only: a {class} on Global.DrippingParticlesPrefab"),
                        _ => {}
                    }
                }
            }
            let (o, k, amount, decay) = at.expect("DrippingParticlesPrefab's DrippingBloodParticleController");
            let objects = prefab.objects.iter().map(|x| (1 + x.components.len(), x.parent)).collect();
            // The ParticleSystem's object and its Transforms up to the root, whose localPosition the Start sets to 0.
            let system = ps.map(|s| {
                let trs = |o: usize| match &prefab.objects[o].components[0].1 {
                    Component::Transform(t) => crate::unity::transform::Trs { t: [t.local_position[0], t.local_position[1], t.local_position[2], 0.0], q: t.local_rotation, s: [t.local_scale[0], t.local_scale[1], t.local_scale[2], 0.0] },
                    _ => unreachable!("a GameObject's first component is its Transform"),
                };
                let mut chain = Vec::new();
                let mut up = Some(s);
                while let Some(x) = up {
                    let mut t = trs(x);
                    if prefab.objects[x].parent.is_none() {
                        t.t = [0.0; 4];
                    }
                    chain.push(t);
                    up = prefab.objects[x].parent;
                }
                (prefab.objects[s].game_object, chain)
            });
            self.v45.drip_prefab = Some((objects, (o, k), amount, decay, system));
        }
    }

    /// A loaded DrippingBloodParticleController's ParticleSystem (its clone's, as dripping_blood_instantiate's).
    pub(crate) fn dripping_blood_loaded(&mut self, d: usize, p: usize, controller: i32) {
        self.drip_prefab();
        if let Some((go, chain)) = self.v45.drip_prefab.clone().unwrap().4 {
            self.embedded_ps.push(super::EmbeddedPs { go, anchor: super::BareAnchor::Physical(p), chain, owner: controller, drive: super::EmbeddedDrive::Drip(d), shape_texture: None, awake: true, emission: None });
        }
    }

    /// PhysicalBehaviour.FixedUpdate's slash drip: LocalDripAreas.Add(point), Colour = colour, Amount = amount.
    pub(crate) fn dripping_blood_drip(&mut self, d: usize, local: [f32; 2], colour: [f32; 4], amount: f32) {
        let x = &mut self.v45.drips[d];
        x.LocalDripAreas.add(local);
        x.Colour = colour;
        x.Amount = amount;
    }

    /// DrippingBloodParticleController.Set
    pub fn dripping_blood_set(&mut self, d: usize, color: [f32; 4], amount: f32) {
        if !(amount <= mathf::EPSILON) {
            let x = &mut self.v45.drips[d];
            for k in 0..4 {
                x.Colour[k] = (x.Colour[k] * x.Amount + color[k] * amount) / (amount + x.Amount);
            }
            x.Amount += amount;
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::game::circulation::liquid_of_class;
    use crate::game::container::Owner;
    use crate::game::Game;
    use crate::play::{new_game, step};

    fn frames(g: &mut Game, n: usize) {
        for _ in 0..n {
            step(g, 1.0 / 60.0).unwrap();
        }
    }

    /// A 1.28.45 game (None with 1.28.41 or without the install).
    fn game45() -> Option<Game> {
        let found = crate::game_assets::find_game(None);
        let game = if std::env::var_os("PPG_ALLOW_1_28_45").is_some() { found.unwrap() } else { found.ok()? };
        // The DLL table (and so the version) before anything reads the version.
        crate::assets::il::load(&game).unwrap();
        let g = new_game(&game, "Default", None, None, 0).unwrap();
        // Run with PPG_ALLOW_1_28_45 set, these tests must not skip silently.
        assert!(std::env::var_os("PPG_ALLOW_1_28_45").is_none() || crate::assets::il::is_1_28_45(), "PPG_ALLOW_1_28_45 set but {game:?} is not 1.28.45");
        crate::assets::il::is_1_28_45().then_some(g)
    }

    /// catalog_spawn of `item` at `at`; its first physical.
    fn spawn(g: &mut Game, item: &str, at: [f32; 3]) -> usize {
        let mut db = g.assets.take().expect("the game's data");
        let first = g.physicals.len();
        let (inst, _) = g.catalog_spawn(&mut db, item, at, false).unwrap_or_else(|e| panic!("{item}: {e}"));
        g.assets = Some(db);
        g.unsynced.push(inst);
        first
    }

    #[test]
    fn n1_format_and_speed_texts() {
        assert_eq!(super::format_n1(0.0), "0.0");
        assert_eq!(super::format_n1(12.34), "12.3");
        assert_eq!(super::format_n1(0.05), "0.1");
        assert_eq!(super::format_n1(9999.0), "9,999.0");
        if game45().is_none() {
            return;
        }
        use super::SpeedUnit as U;
        assert_eq!(super::convert_speed(10.0, U::KilometersPerHour), "36.0 km/s");
        assert_eq!(super::convert_speed(10.0, U::MetersPerSecond), "10.0 m/s");
        assert_eq!(super::convert_speed(1.0, U::MilesPerHour), "2.2 mph");
        assert_eq!(super::convert_speed(1.0, U::FeetPerSecond), "3.3 ft/s");
        assert_eq!([U::MetersPerSecond, U::KilometersPerHour, U::FeetPerSecond, U::MilesPerHour].map(U::display), ["m/s", "km/h", "ft/s", "mph"]);
        assert_eq!(crate::il45!(str "SpeedometerBehaviour::Start" 0), "resetTopSpeed");
        assert_eq!(crate::il45!(str "SpeedometerBehaviour::SetText" 0), "\n<color=blue>");
    }

    #[test]
    fn new_items_spawn_and_run() {
        let Some(mut g) = game45() else { return };
        for (k, item) in ["Buzzer", "Speedometer", "Crossbow", "Crossbow Bolt"].into_iter().enumerate() {
            spawn(&mut g, item, [k as f32 * 2.0 - 3.0, 3.0, 0.0]);
        }
        frames(&mut g, 120);
        assert_eq!((g.v45.buzzers.len(), g.v45.speedometers.len(), g.v45.crossbows.len()), (1, 1, 1));
    }

    #[test]
    fn speedometer_reads_a_fall_and_toggles() {
        let Some(mut g) = game45() else { return };
        let p = spawn(&mut g, "Speedometer", [0.0, 8.0, 0.0]);
        // The real 1.28.45 fall (reference-probe/game/v45vis_1.txt): speedMs after 1, 3, 5, 7, 8, 10 fixed steps of free fall.
        let mut seen = Vec::new();
        for _ in 0..15 {
            frames(&mut g, 1);
            seen.push(g.v45.speedometers[0].speedMs.to_bits());
        }
        for real in [0x3e258af7u32, 0x3ef85072, 0x3f4eedb5, 0x3f90d998, 0x3fa58af7, 0x3fceedb5] {
            assert!(seen.contains(&real), "{real:08x} not in {seen:08x?}");
        }
        g.speedometer_set_text(0);
        let t0 = g.v45.speedometers[0].TextMesh.expect("the TextMeshPro");
        let s0 = &g.v45.speedometers[0];
        assert_eq!(g.world_texts[t0].text, format!("{:.1} m/s
<color=blue>{:.1} m/s", s0.speedMs, s0.TopSpeedMs));
        // OnDisable: the text and the light off; OnEnable (UpdateActivation): back on (the real log's "disabled" and "enabled" lines).
        g.speedometer_on_disable(0);
        let l0 = g.v45.speedometers[0].Light.expect("the Light");
        assert!(!g.world_texts[t0].enabled && !g.sprite_drawn(l0));
        g.speedometer_update_activation(0);
        assert!(g.world_texts[t0].enabled && g.sprite_drawn(l0));
        frames(&mut g, 18);
        let s = &g.v45.speedometers[0];
        assert!(s.speedMs > 1.0 && s.TopSpeedMs >= s.speedMs, "{} {}", s.speedMs, s.TopSpeedMs);
        g.speedometer_set_text(0);
        let t = g.v45.speedometers[0].TextMesh.expect("the TextMeshPro");
        let light = g.v45.speedometers[0].Light.expect("the Light");
        assert!(g.world_texts[t].text.contains(" m/s\n<color=blue>"), "{}", g.world_texts[t].text);
        assert!(g.world_texts[t].enabled && g.sprite_drawn(light));
        assert!(g.physicals[p].menu.iter().any(|b| b.Identity == "resetTopSpeed"));
        g.activate_for_test(p);
        frames(&mut g, 1);
        assert!(!g.v45.speedometers[0].Activated);
        // UpdateActivation: the text and the light go off with it.
        assert!(!g.world_texts[t].enabled && !g.sprite_drawn(light));
    }

    /// PhysicalBehaviour.Start: a slashing item (SlashColliders) gets a DrippingParticlesPrefab clone that runs.
    #[test]
    fn slashing_items_get_dripping_blood() {
        let Some(mut g) = game45() else { return };
        g.prefs.FancyEffects = true;
        for name in ["Sword", "Knife", "Axe"] {
            spawn(&mut g, name, [0.0, 2.0, 0.0]);
        }
        frames(&mut g, 2);
        let with: Vec<usize> = (0..g.physicals.len()).filter(|&p| g.physicals[p].DrippingBloodParticles.is_some()).collect();
        assert!(!with.is_empty() && with.len() == g.v45.drips.len(), "{with:?}");
        let d = g.physicals[with[0]].DrippingBloodParticles.unwrap();
        assert_eq!(g.v45.drips[d].PhysicalBehaviour, Some(with[0]));
        g.dripping_blood_drip(d, [0.0, 0.0], [1.0, 0.0, 0.0, 1.0], 1.0);
        frames(&mut g, 10);
        assert!(!g.v45.drips[d].emitted.is_empty());
    }

    #[test]
    fn buzzer_buzzes_on_use() {
        let Some(mut g) = game45() else { return };
        let p = spawn(&mut g, "Buzzer", [0.0, 2.0, 0.0]);
        frames(&mut g, 10);
        let d = g.v45.buzzers[0].Diaphragm.expect("the Diaphragm's sprite");
        assert_eq!(g.sprite_local_scale(d), [1.0; 3]);
        g.activate_for_test(p);
        frames(&mut g, 1);
        assert!(g.v45.buzzers[0].lastPlayedTime > 0.0);
        // OnWillRenderObject: the diaphragm swells with the buzz's output, then rests at 1 once it stops.
        let mut most = 0.0f32;
        for _ in 0..30 {
            frames(&mut g, 1);
            most = most.max(g.sprite_local_scale(d)[1]);
        }
        // The real 1.28.45 buzz (reference-probe/game/v45vis_1.txt, tools/recorder V45VisProbe.cs): y peaks at 1.0329 (3f84371b),
        // x and z stay 1. The samples follow the audio thread's timing, so only the range is compared.
        assert!(most > 1.0 && most < 1.1, "{most}");
        frames(&mut g, 300);
        assert_eq!(g.sprite_local_scale(d), [1.0; 3]);
    }

    #[test]
    fn crossbow_bolt_carries_the_syringe_liquid_into_a_limb() {
        let Some(mut g) = game45() else { return };
        let bow = spawn(&mut g, "Crossbow", [0.0, 2.0, 0.0]);
        let s = g.syringes.len();
        spawn(&mut g, "Life Syringe", [3.0, 2.0, 0.0]);
        let human = spawn(&mut g, "Human", [8.0, 3.0, 0.0]);
        frames(&mut g, 10);
        g.crossbow_connect_syringe(0, s);
        frames(&mut g, 10);
        assert_eq!(g.v45.crossbows[0].AttachedSyringe, Some(s));
        let before = g.bc(Owner::Syringe(s)).TotalLiquidAmount;
        g.activate_for_test(bow);
        frames(&mut g, 5);
        let x = g.v45.injectables.first().expect("the bolt's InjectableBehaviour").clone();
        // InjectableBehaviour.Start found the bolt's imbuefx system and played it.
        assert!(x.ImbueParticles.is_some());
        // The real 1.28.45 bolt (reference-probe/game/v45vis_1.txt): FxColor (and the imbuefx startColor) is the Life Syringe's
        // computed colour with alpha 0.02.
        assert_eq!(x.FxColor.map(f32::to_bits), [0x3f2cade3, 0x3f800000, 0x3f487331, 0x3ca3d70a]);
        let life = liquid_of_class("LifeSerumLiquid");
        let carried: f32 = x.LiquidDistributions.as_ref().unwrap().iter().map(|&(_, a)| a).sum();
        // The syringe is infinite (refills in FixedUpdate), so compare with SyringeFluidUsage, not what is left.
        let usage = g.v45.crossbows[0].SyringeFluidUsage;
        assert!(carried > 0.0 && (carried - before.min(usage)).abs() < 1e-4, "{carried} {before} {usage}");
        let limb = g.limbs.iter().position(|l| l.PhysicalBehaviour >= human).unwrap();
        let had = g.get_amount(Owner::Circulation(limb), life);
        g.injectable_lodged_on(x.phys.unwrap(), g.limbs[limb].PhysicalBehaviour);
        assert!(g.get_amount(Owner::Circulation(limb), life) > had);
        assert!(g.v45.injectables[0].destroyed);
    }

    /// The 1.28.45 recipes (Global.Awake's LiquidMixInstructions with ContainerFilter) in a syringe.
    #[test]
    fn new_mix_recipes_make_their_liquids() {
        let Some(mut g) = game45() else { return };
        spawn(&mut g, "Life Syringe", [0.0, 2.0, 0.0]);
        frames(&mut g, 5);
        let o = Owner::Syringe(0);
        for (sources, target) in [
            (&["CirculationPoison", "ZombiePoisonLiquid", "FreezePoisonLiquid", "InstantDeathPoisonLiquid"][..], "Nasumica"),
            (&["Coolant", "ExoticLiquid", "MendingSerum"][..], "Amaraevelum"),
            (&["Nasumica", "Amaraevelum"][..], "Dulcamarus"),
        ] {
            g.clear_liquid(o);
            for s in sources {
                g.add_liquid(o, liquid_of_class(s), 0.3);
            }
            frames(&mut g, 150);
            assert!(g.get_amount(o, liquid_of_class(target)) > 0.0, "{target}");
        }
    }

    #[test]
    fn nasumica_cures_a_zombie_limb() {
        let Some(mut g) = game45() else { return };
        let human = spawn(&mut g, "Human", [0.0, 3.0, 0.0]);
        frames(&mut g, 10);
        let l = g.limbs.iter().position(|l| l.PhysicalBehaviour >= human).unwrap();
        g.limbs[l].IsZombie = true;
        g.add_liquid(Owner::Circulation(l), liquid_of_class("Nasumica"), 0.5);
        frames(&mut g, 5);
        assert!(!g.limbs[l].IsZombie);
    }

    #[test]
    fn dulcamarus_heals_by_siphoning_a_neighbour() {
        let Some(mut g) = game45() else { return };
        let a = spawn(&mut g, "Human", [0.0, 3.0, 0.0]);
        let b = spawn(&mut g, "Human", [1.0, 3.0, 0.0]);
        frames(&mut g, 30);
        let la = g.limbs.iter().position(|l| l.PhysicalBehaviour >= a && l.HasBrain).unwrap();
        let person_b = g.limbs[g.limbs.iter().position(|l| l.PhysicalBehaviour >= b).unwrap()].Person;
        g.add_liquid(Owner::Circulation(la), liquid_of_class("Dulcamarus"), 0.5);
        frames(&mut g, 2);
        assert_eq!(g.v45.dulcamarus.len(), 1);
        g.limbs[la].Health = g.limbs[la].InitialHealth * 0.5;
        frames(&mut g, 150);
        assert_eq!(g.limbs[la].Health, g.limbs[la].InitialHealth);
        // One siphon pass heals at once, so the drain is tiny and B's regeneration outruns it: check the sources instead.
        let sources: Vec<_> = g.v45.dulcamarus[0].siphonSources.iter().flatten().map(|s| s.Limb).collect();
        assert!(!sources.is_empty() && sources.iter().all(|&l| g.limbs[l].Person == person_b), "{sources:?}");
        // Some sources got a tendril LineRenderer in `lines` (DivineLight, world space, 12 points), drawn while enabled.
        let tendrils: Vec<usize> = g.v45.dulcamarus[0].siphonSources.iter().flatten().filter_map(|s| s.LineRenderer.as_ref().map(|t| t.line)).collect();
        for &l in &tendrils {
            let line = &g.lines[l];
            assert!(line.use_world_space && line.positions.len() == 12 && line.material.is_some() && line.enabled && line.visible);
        }
    }

    /// MagneticBehaviour (no 1.28.45 prefab has one): a magnet on one brick pulls a nearby iron brick and is pulled back.
    #[test]
    fn magnetic_behaviour_attracts_metal() {
        let Some(mut g) = game45() else { return };
        let a = spawn(&mut g, "Pistol", [0.0, 10.0, 0.0]);
        let b = spawn(&mut g, "Pistol", [3.0, 10.0, 0.0]);
        frames(&mut g, 1);
        for p in [a, b] {
            g.physicals[p].rigidbody.set_gravity_scale(&mut g.world, 0.0);
        }
        let id = g.allocate_instance_id();
        g.v45.magnets.push(super::Magnetic { id, phys: a, EffectiveDistance: 6.0, NorthPole: [0.0, 0.2], SouthPole: [0.0, -0.2], MagnetStrength: 20.0, ..Default::default() });
        let x0 = (g.physicals[a].transform.position[0], g.physicals[b].transform.position[0]);
        for _ in 0..20 {
            g.magnetic_fixed_update(0);
            frames(&mut g, 1);
        }
        let x1 = (g.physicals[a].transform.position[0], g.physicals[b].transform.position[0]);
        assert!(x1.0 > x0.0 && x1.1 < x0.1, "{x0:?} -> {x1:?}");
        assert!(super::magnetic_get_pole_sign(super::Pole::North, super::Pole::North) < 0.0);
        assert_eq!(super::magnetic_get_force(1.0, [0.0, 0.0], [0.05, 0.0]), [0.0, 0.0]);
    }

    /// CrossbowBehaviour: OnPreFlip parents the syringe for the frame, a drag of the bow parents it until the drag ends,
    /// and DisconnectSyringe sets its layer 9 only after a second.
    #[test]
    fn crossbow_flip_drag_and_delayed_layer() {
        let Some(mut g) = game45() else { return };
        let bow = spawn(&mut g, "Crossbow", [0.0, 2.0, 0.0]);
        let s = g.syringes.len();
        spawn(&mut g, "Life Syringe", [3.0, 2.0, 0.0]);
        frames(&mut g, 10);
        g.crossbow_connect_syringe(0, s);
        frames(&mut g, 2);
        let sp = g.syringes[s].Phys;
        g.crossbow_on_pre_flip(0);
        assert!(g.physical_is_child_of(sp, bow));
        frames(&mut g, 1);
        assert!(!g.physical_is_child_of(sp, bow), "unparented at the end of the frame");
        g.crossbow_on_drag(bow, false);
        assert!(g.physical_is_child_of(sp, bow));
        g.crossbow_on_drag(bow, true);
        assert!(!g.physical_is_child_of(sp, bow));
        assert_eq!(g.physical_layer(sp), 10);
        g.crossbow_disconnect_syringe(0);
        frames(&mut g, 30);
        assert_eq!(g.physical_layer(sp), 10, "not before the second");
        frames(&mut g, 40);
        assert_eq!(g.physical_layer(sp), 9);
    }

    /// The saved members of the new classes come back through a contraption's bytes (ObjectStates, Ceras, paste).
    #[test]
    fn new_classes_round_trip_through_a_save() {
        let Some(mut g) = game45() else { return };
        let buzzer = spawn(&mut g, "Buzzer", [0.0, 2.0, 0.0]);
        let meter = spawn(&mut g, "Speedometer", [2.0, 2.0, 0.0]);
        let bow = spawn(&mut g, "Crossbow", [4.0, 2.0, 0.0]);
        let s = g.syringes.len();
        let syringe = spawn(&mut g, "Life Syringe", [6.0, 2.0, 0.0]);
        let human = spawn(&mut g, "Human", [-6.0, 3.0, 0.0]);
        frames(&mut g, 10);
        g.crossbow_connect_syringe(0, s);
        let head = g.limbs.iter().position(|l| l.PhysicalBehaviour >= human && l.HasBrain).unwrap();
        g.add_liquid(Owner::Circulation(head), liquid_of_class("Dulcamarus"), 0.5);
        frames(&mut g, 5);
        // Buzzer audio: 8 kHz unsigned 8-bit samples.
        let audio: Vec<u8> = (0..4000).map(|i| (128.0 + 100.0 * (i as f32 * 0.3).sin()) as u8).collect();
        let (_, data_id) = g.buzzer_clip_from_data(audio.clone());
        g.v45.buzzers[0].CurrentDataId = data_id;
        g.v45.buzzers[0].Loudness = 7.5;
        g.v45.speedometers[0].TopSpeedMs = 12.5;
        g.v45.crossbows[0].SyringeEjectForce = 3.25;
        g.v45.dulcamarus[0].Radius = 4.5;
        // An InjectableBehaviour as OnLaunch leaves it on a bolt.
        let bolt = spawn(&mut g, "Crossbow Bolt", [8.0, 2.0, 0.0]);
        frames(&mut g, 2);
        let id = g.allocate_instance_id();
        g.v45.injectables.push(super::Injectable { id, phys: Some(bolt), LiquidDistributions: Some(vec![("LIFE SERUM".into(), 0.25)]), FxColor: [0.5, 0.25, 1.0, 1.0], destroyed: false, ImbueParticles: None });
        g.injectable_attach(g.v45.injectables.len() - 1, true);
        let humans: Vec<usize> = (human..g.physicals.len()).filter(|&p| g.limbs.iter().any(|l| l.PhysicalBehaviour == p)).collect();
        let mut picked = vec![buzzer, meter, bow, syringe, bolt];
        picked.extend(humans.iter().take(1));
        let states = g.object_states_of(&picked, [0.0; 3]);
        let bytes = crate::game::contraption::serialise_states(&states).unwrap();
        let states = crate::game::contraption::deserialise_states(&bytes).unwrap();
        let counts = (g.v45.buzzers.len(), g.v45.speedometers.len(), g.v45.crossbows.len(), g.v45.dulcamarus.len(), g.v45.injectables.len());
        g.paste_states(&states, [0.0, 12.0, 0.0], false, "Paste");
        frames(&mut g, 3);
        let b = &g.v45.buzzers[counts.0];
        assert_eq!((b.Loudness, b.CurrentDataId), (7.5, data_id), "the buzzer's Loudness and its audio (SerialisableAudioData)");
        assert!(b.clip.is_some() && b.SerialisableAudioData.is_none());
        assert_eq!(g.v45.speedometers[counts.1].TopSpeedMs, 12.5);
        assert_eq!(g.v45.crossbows[counts.2].SyringeEjectForce, 3.25);
        assert!(g.v45.crossbows[counts.2].AttachedSyringe.is_some_and(|x| x != s), "AttachedSyringe links to the pasted syringe");
        assert_eq!(g.v45.dulcamarus.len(), counts.3 + 1);
        assert_eq!(g.v45.dulcamarus[counts.3].Radius, 4.5);
        assert_eq!(g.v45.injectables.len(), counts.4 + 1);
        let x = &g.v45.injectables[counts.4];
        assert_eq!((x.LiquidDistributions.clone(), x.FxColor), (Some(vec![("LIFE SERUM".to_string(), 0.25)]), [0.5, 0.25, 1.0, 1.0]));
    }

    /// BuzzerBehaviour: ClipFromData caches by xxHash, OnBeforeSerialise writes the data back, OpenFile converts a WAVE file
    /// and refuses one over 1 MB.
    #[test]
    fn buzzer_audio_data_and_open_file() {
        let Some(mut g) = game45() else { return };
        let p = spawn(&mut g, "Buzzer", [0.0, 2.0, 0.0]);
        frames(&mut g, 3);
        assert!(g.physicals[p].menu.iter().any(|b| b.Identity == "chooseAudioClip") && g.physicals[p].menu.iter().any(|b| b.Identity == "setLoudness"));
        let data: Vec<u8> = (0..800).map(|i| (i % 256) as u8).collect();
        let (c1, id) = g.buzzer_clip_from_data(data.clone());
        let (c2, id2) = g.buzzer_clip_from_data(data.clone());
        assert!(id == id2 && std::sync::Arc::ptr_eq(&c1, &c2) && id == crate::game::contraption::xxhash(&data, 0));
        // 8-bit sample 0 is -1.0, 128 is 0.0.
        let pcm = c1.pcm.as_ref().unwrap();
        assert_eq!((pcm.frequency, pcm.data.len(), pcm.data[0], pcm.data[128]), (8000, 800, -32768, 0));
        g.v45.buzzers[0].CurrentDataId = id;
        g.buzzer_on_before_serialise(0);
        assert_eq!(g.v45.buzzers[0].SerialisableAudioData.as_deref(), Some(&data[..]));
        // A 0.5 s 16-bit 22050 Hz mono WAVE file: 4000 samples at 8 kHz.
        let dir = std::env::temp_dir().join(format!("ppg-buzzer-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = |n: usize| {
            let mut v = Vec::new();
            let len = (n * 2) as u32;
            v.extend(b"RIFF");
            v.extend((36 + len).to_le_bytes());
            v.extend(b"WAVEfmt ");
            v.extend(16u32.to_le_bytes());
            v.extend(1u16.to_le_bytes());
            v.extend(1u16.to_le_bytes());
            v.extend(22050u32.to_le_bytes());
            v.extend(44100u32.to_le_bytes());
            v.extend(2u16.to_le_bytes());
            v.extend(16u16.to_le_bytes());
            v.extend(b"data");
            v.extend(len.to_le_bytes());
            for i in 0..n {
                v.extend((((i as f32 * 0.1).sin() * 12000.0) as i16).to_le_bytes());
            }
            v
        };
        let small = dir.join("small.wav");
        std::fs::write(&small, wav(11025)).unwrap();
        g.buzzer_open_file_callback(0, &[small.to_string_lossy().into_owned()], None);
        let id = g.v45.buzzers[0].CurrentDataId;
        assert!(id != 0 && g.v45.buzzers[0].clip.is_some());
        let n = g.v45.buzzer_data_cache[&id].len();
        assert!((3990..=4000).contains(&n), "{n}");
        assert_ne!(g.v45.buzzers[0].color, [1.0; 4]);
        let big = dir.join("big.wav");
        std::fs::write(&big, wav(600_000)).unwrap();
        let path = big.to_string_lossy().into_owned();
        let ex = crate::game::menu::catch_managed(|| g.buzzer_open_file_callback(0, &[path], None));
        assert_eq!(ex.map(|e| e.0), Some("PpgPlayerException: File size may not exceed 1MB".to_string()));
        let _ = std::fs::remove_file(&small);
        let _ = std::fs::remove_file(&big);
        let _ = std::fs::remove_dir(&dir);
    }

    /// Preferences.SpeedUnit: read from config.json by name, used by the speedometer text, listed after TemperatureUnit.
    #[test]
    fn speed_unit_preference() {
        let Some(mut g) = game45() else { return };
        let prefs = crate::game::Prefs::from_json("{\"SpeedUnit\": \"MilesPerHour\"}").unwrap();
        assert_eq!(prefs.SpeedUnit, 3);
        g.prefs.SpeedUnit = 3;
        spawn(&mut g, "Speedometer", [0.0, 2.0, 0.0]);
        frames(&mut g, 2);
        g.speedometer_set_text(0);
        let t = g.v45.speedometers[0].TextMesh.unwrap();
        assert!(g.world_texts[t].text.ends_with(" mph"), "{}", g.world_texts[t].text);
        let rows = crate::ui::tables::SETTINGS.iter().position(|s| s.field == "SpeedUnit").expect("a SpeedUnit setting");
        assert_eq!(crate::ui::tables::SETTINGS[rows].title, "Speed unit");
        let mut s = crate::ui::state::Settings::default();
        s.load("{\"SpeedUnit\": \"FeetPerSecond\"}");
        assert_eq!(s.text(rows), "ft/s");
        let j = s.json();
        let (t, u) = (j.find("\"TemperatureUnit\"").unwrap(), j.find("\"SpeedUnit\": \"FeetPerSecond\"").expect("written by name"));
        assert!(t < u && !j[t..u].contains("VSync"), "right after TemperatureUnit");
    }

    /// DrippingBloodParticleController.Update: a drip lands on a SlashColliders collider (ClosestPoint) and is emitted on
    /// the clone's ParticleSystem.
    #[test]
    fn drips_land_on_the_slash_collider() {
        let Some(mut g) = game45() else { return };
        g.prefs.FancyEffects = true;
        let first = spawn(&mut g, "Sword", [0.0, 6.0, 0.0]);
        frames(&mut g, 2);
        // Held still in the air, so each drip can be checked against where the blade is now.
        for q in first..g.physicals.len() {
            g.physicals[q].rigidbody.set_gravity_scale(&mut g.world, 0.0);
        }
        let p = (first..g.physicals.len()).find(|&q| g.physicals[q].DrippingBloodParticles.is_some()).unwrap();
        let d = g.physicals[p].DrippingBloodParticles.unwrap();
        assert!(g.embedded_ps.iter().any(|e| matches!(e.drive, crate::game::EmbeddedDrive::Drip(x) if x == d)));
        // Far off the blade: each drip is pulled onto the slash collider.
        g.dripping_blood_drip(d, [0.0, 3.0], [1.0, 0.0, 0.0, 1.0], 1.0);
        frames(&mut g, 10);
        let cs = g.physicals[p].SlashColliders.clone().unwrap();
        let drips = g.v45.drips[d].emitted.clone();
        assert!(!drips.is_empty());
        for (at, _) in drips {
            let on = cs.iter().any(|&c| {
                let q = g.world.collider_closest_point(c, crate::b2::math::Vec2 { x: at[0], y: at[1] });
                (q.x - at[0]).abs() < 0.05 && (q.y - at[1]).abs() < 0.05
            });
            assert!(on, "{at:?} is off the blade");
        }
    }
    use crate::assets::db::{AssetDb, Component};
    use crate::assets::mono::Value;

    /// Survey: the new classes' objects in the install's files, and every catalog item (`-- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn v45_survey() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        for class in ["BuzzerBehaviour", "CrossbowBehaviour", "DulcamarusBehaviour", "MagneticBehaviour", "SpeedometerBehaviour", "InjectableBehaviour", "DrippingBloodParticleController", "ProjectileLauncherBehaviour", "Global"] {
            for r in db.find_scripts(class).unwrap() {
                let (_, mb, v) = db.mono(r).unwrap().unwrap();
                let s = format!("{v:?}");
                let go = db.resolve(r.file, mb.game_object).map(|g| db.game_object(g).map(|x| x.name.clone()).unwrap_or_default());
                println!("GO {class} {go:?}");
                println!("CLASS {class} file {} {} go {:?}: {}", r.file, db.files[r.file].path.display(), mb.game_object, &s[..s.len().min(1500)]);
            }
        }
        for (path, r) in db.resource_entries(1).unwrap() {
            if ["buzz", "speed", "magnet", "divine", "drip", "crossbow", "bolt"].iter().any(|k| path.contains(k)) {
                println!("RES {path} {r:?}");
            }
        }
        for n in crate::game::Game::catalog_names(&mut db).unwrap() {
            if let Ok(p) = crate::game::Game::catalog_prefab(&mut db, &n) {
                let classes: Vec<String> = p.objects.iter().flat_map(|o| o.components.iter().filter_map(|(_, c)| match c { Component::Script { class, .. } => Some(class.clone()), _ => None })).collect();
                println!("SPAWNABLE {n} | {}", classes.join(" "));
            }
        }
        for n in ["Buzzer", "Speedometer", "Crossbow", "Crossbow Bolt"] {
            let p = crate::game::Game::catalog_prefab(&mut db, n).unwrap();
            for (i, o) in p.objects.iter().enumerate() {
                println!("DUMP {n} OBJ {i} {} layer {} active {} parent {:?}", o.name, o.layer, o.active, o.parent);
                for (r, c) in &o.components {
                    match c {
                        Component::Script { class, enabled, fields, .. } => { let s = format!("{fields:?}"); println!("DUMP   SCRIPT {class} {enabled} {:?} {}", r, &s[..s.len().min(400)]) }
                        Component::Other(id) => println!("DUMP   OTHER {id} {r:?}"),
                        x => println!("DUMP   {:?} {r:?}", std::mem::discriminant(x)),
                    }
                }
            }
        }
        let catalog = db.find_scripts("CatalogData").unwrap()[0];
        let (_, _, catalog_data) = db.mono(catalog).unwrap().unwrap();
        for item in catalog_data.get("Items").and_then(Value::array).unwrap().to_vec() {
            let asset = db.resolve(catalog.file, item.pptr().unwrap()).unwrap();
            let (_, _, spawnable) = db.mono(asset).unwrap().unwrap();
            let Some(prefab) = spawnable.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(asset.file, p)) else { continue };
            let p = db.prefab(prefab).unwrap();
            let classes: Vec<String> = p.objects.iter().flat_map(|o| o.components.iter().filter_map(|(_, c)| match c { Component::Script { class, .. } => Some(class.clone()), _ => None })).collect();
            println!("ITEM {} | {}", p.objects[0].name, classes.join(" "));
        }
    }
}
