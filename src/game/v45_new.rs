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
pub const LINKED: &[&str] = &["SpeedometerBehaviour", "CrossbowBehaviour", "BuzzerBehaviour", "DrippingBloodParticleController"];

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
}

impl V {
    pub fn class(self) -> &'static str {
        match self {
            V::Speedometer(_) => "SpeedometerBehaviour",
            V::Crossbow(_) => "CrossbowBehaviour",
            V::Buzzer(_) => "BuzzerBehaviour",
            V::Dulcamarus(_) => "DulcamarusBehaviour",
            V::DrippingBlood(_) => "DrippingBloodParticleController",
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
    /// CircularBuffer..ctor(size)
    pub fn new(size: usize) -> Self {
        CircularBuffer { Data: vec![T::default(); size], WriteIndex: 0 }
    }
    /// CircularBuffer.Add
    pub fn add(&mut self, value: T) {
        let n = self.Data.len() as i32;
        self.Data[(self.WriteIndex % n) as usize] = value;
        self.WriteIndex += 1;
    }
    /// CircularBuffer.PickRandom: Random.Range(0, WriteIndex - 1) % Length (default before the first Add).
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
    /// Light.enabled and TextMesh.enabled (UpdateActivation, OnDisable).
    pub display_on: bool,
    /// TextMesh.text as SetText last wrote it.
    pub text: String,
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
    /// LiquidDistributions: (liquid, amount); None once spent (null).
    pub LiquidDistributions: Option<Vec<(usize, f32)>>,
    pub FxColor: [f32; 4],
    pub destroyed: bool,
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
    /// LineRenderer ("tendril"): widthMultiplier, start/end colour, the 12 positions, enabled (rendering).
    pub LineRenderer: Option<Tendril>,
}

#[derive(Clone, Debug, Default)]
pub struct Tendril {
    pub id: i32,
    pub width: f32,
    pub start_color: [f32; 4],
    pub end_color: [f32; 4],
    pub positions: Vec<[f32; 3]>,
    pub enabled: bool,
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

/// The new classes' arenas.
#[derive(Default)]
pub struct V45 {
    pub speedometers: Vec<Speedometer>,
    pub crossbows: Vec<Crossbow>,
    pub injectables: Vec<Injectable>,
    pub buzzers: Vec<Buzzer>,
    pub dulcamarus: Vec<Dulcamarus>,
    pub drips: Vec<DrippingBlood>,
    /// UserPreferenceManager.Current.SpeedUnit (Preferences.SpeedUnit).
    pub speed_unit: SpeedUnit,
    /// Disconnected syringes' Utils.DelayCoroutine(1f, SetLayer(9)): (due time, physical).
    pub delayed_layers: Vec<(f32, usize)>,
    /// Global.DrippingParticlesPrefab: each object's (ids it takes, parent), the controller's (object, component), and
    /// its Amount and DecaySpeed.
    pub drip_prefab: Option<(Vec<(usize, Option<usize>)>, (usize, usize), f32, f32)>,
}

/// A script's PPtr field `name` as the prefab object it points at (None: null).
fn field_object(prefab: &Prefab, o: usize, v: &Value, name: &str) -> Option<usize> {
    let p = v.get(name)?.pptr()?;
    if p.path_id == 0 {
        return None;
    }
    let file = prefab.objects[o].game_object.file;
    prefab.index.get(&ObjRef { file, path_id: p.path_id }).map(|&(k, _)| k).filter(|_| p.file == 0)
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
    pub(super) fn v45_link(&mut self, prefab: &Prefab, inst: &super::spawn::Instance, linked: &super::link::Linked, o: usize, c: usize) -> Option<Behaviour> {
        let Component::Script { class, enabled, fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
        let id = inst.objects[o].components[c];
        let enabled = *enabled;
        let physical_at = |k: Option<usize>| k.and_then(|k| linked.physicals.get(&k).copied());
        let b = match class.as_str() {
            "SpeedometerBehaviour" => {
                let Phys = physical_at(field_object(prefab, o, v, "Phys")).unwrap_or_else(|| unimplemented!("mod-only: SpeedometerBehaviour.Phys null"));
                self.v45.speedometers.push(Speedometer { id, enabled, Phys, Activated: v.get("Activated").and_then(Value::bool).unwrap_or(true), TopSpeedMs: f(v, "TopSpeedMs", 0.0), ..Default::default() });
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
                // Awake: Launcher.OnLaunch += OnLaunch (launcher.rs calls crossbow_on_launch).
                V::Crossbow(self.v45.crossbows.len() - 1)
            }
            "BuzzerBehaviour" => {
                // Awake: PhysicalBehaviour and SpriteRenderer default to this object's.
                let p = physical_at(field_object(prefab, o, v, "PhysicalBehaviour")).or_else(|| linked.physicals.get(&o).copied()).unwrap_or_else(|| unimplemented!("mod-only: BuzzerBehaviour without a PhysicalBehaviour"));
                let damagable = field_object(prefab, o, v, "DamagableBehaviour").and_then(|k| linked.machinery.get(&k).copied());
                self.v45.buzzers.push(Buzzer {
                    id,
                    enabled,
                    PhysicalBehaviour: p,
                    DamagableBehaviour: damagable,
                    AudioData: v.get("AudioData").and_then(|s| if let Value::Str(s) = s { Some(s.clone()) } else { None }).unwrap_or_default(),
                    Loudness: f(v, "Loudness", 1.0),
                    lastPlayedTime: f32::NEG_INFINITY,
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
            _ => unreachable!(),
        };
        Some(Behaviour::V45(b))
    }

    /// The Start a new class's component queues at link (None: it declares none).
    pub(super) fn v45_pending_start(&self, id: i32) -> Option<super::misc::PendingStart> {
        match self.behaviour_of.get(&id) {
            Some(&(_, Behaviour::V45(v @ (V::Speedometer(_) | V::Buzzer(_))))) => Some(super::misc::PendingStart::V45(v)),
            _ => None,
        }
    }

    pub(super) fn v45_start(&mut self, v: V) {
        match v {
            V::Speedometer(s) => self.speedometer_start(s),
            V::Buzzer(b) => self.buzzer_start(b),
            V::Dulcamarus(d) => self.dulcamarus_start(d),
            _ => {}
        }
    }

    /// The managers' callbacks.
    pub(super) fn v45_phase(&mut self, phase: super::Phase, v: V) {
        use super::Phase;
        match (phase, v) {
            (Phase::Fixed, V::Speedometer(s)) => self.speedometer_fixed_update(s),
            (Phase::Update, V::Crossbow(c)) => self.crossbow_update(c),
            (Phase::Update, V::Dulcamarus(d)) => self.dulcamarus_update(d),
            (Phase::Update, V::DrippingBlood(d)) => self.dripping_blood_update(d),
            (phase, v) => unreachable!("{v:?} in the {phase:?} manager"),
        }
    }

    /// Messages.IUse.Use
    pub(super) fn v45_use(&mut self, v: V) {
        match v {
            V::Speedometer(s) => self.speedometer_use(s),
            V::Buzzer(b) => self.buzzer_use(b),
            _ => unreachable!(),
        }
    }

    // ---- SpeedometerBehaviour ------------------------------------------------------------------------------------

    /// SpeedometerBehaviour.Start: UpdateActivation, then the "resetTopSpeed" button (shown while TopSpeedMs > 0).
    fn speedometer_start(&mut self, s: usize) {
        self.speedometer_update_activation(s);
        let p = self.v45.speedometers[s].Phys;
        let label = crate::il45!(str "SpeedometerBehaviour::Start" 1);
        let button = super::menu::MenuButton { condition: super::menu::Cond::SpeedometerRecord(s), ..super::menu::MenuButton::new("resetTopSpeed", label, crate::il45!(str "SpeedometerBehaviour::Start" 2), super::menu::MenuAction::SpeedometerReset(s)) };
        self.physicals[p].menu.push(button);
    }

    /// SpeedometerBehaviour.Use
    fn speedometer_use(&mut self, s: usize) {
        if self.v45.speedometers[s].enabled {
            self.v45.speedometers[s].Activated = !self.v45.speedometers[s].Activated;
            self.speedometer_update_activation(s);
        }
    }

    /// SpeedometerBehaviour.UpdateActivation (also OnEnable): Light and TextMesh shown while Activated.
    fn speedometer_update_activation(&mut self, s: usize) {
        let x = &mut self.v45.speedometers[s];
        x.display_on = x.Activated;
    }

    /// SpeedometerBehaviour.FixedUpdate
    fn speedometer_fixed_update(&mut self, s: usize) {
        let p = self.v45.speedometers[s].Phys;
        let v = self.physicals[p].rigidbody.velocity(&self.world);
        let x = &mut self.v45.speedometers[s];
        x.speedMs = mathf::magnitude2(v) * 0.82397;
        x.TopSpeedMs = mathf::max(x.TopSpeedMs, x.speedMs);
    }

    /// SpeedometerBehaviour.OnWillRenderObject: SetText.
    pub fn speedometer_set_text(&mut self, s: usize) {
        let unit = self.v45.speed_unit;
        let x = &mut self.v45.speedometers[s];
        let tag = crate::il45!(str "SpeedometerBehaviour::SetText" 0);
        x.text = format!("{}{tag}{}", convert_speed(x.speedMs, unit), convert_speed(x.TopSpeedMs, unit));
    }

    // ---- CrossbowBehaviour and InjectableBehaviour ---------------------------------------------------------------

    /// The crossbows that take a physical's collision messages: (component index, CollisionScript).
    pub(super) fn v45_collision_scripts(&self, p: usize, after: bool) -> Vec<(usize, super::acts::CollisionScript)> {
        if !crate::assets::il::is_1_28_45() {
            return Vec::new();
        }
        (0..self.v45.crossbows.len()).filter(|&c| self.v45.crossbows[c].phys == p && self.v45.crossbows[c].after_physical == after).map(|c| (self.v45.crossbows[c].index, super::acts::CollisionScript::Crossbow(c))).collect()
    }

    /// CrossbowBehaviour.OnCollisionEnter2D
    pub(super) fn crossbow_on_collision_enter(&mut self, c: usize, col: &crate::b2::contacts2d::ScriptCollision) {
        let x = &self.v45.crossbows[c];
        if x.hasSyringe || x.syringeJoint.is_some() || x.AttachedSyringe.is_some() || self.time.time - x.lastDisconnectTime < 1.0 {
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
    fn crossbow_connect_syringe(&mut self, c: usize, s: usize) {
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
        // syringe.gameObject.SetLayer(10)
        self.v45_set_layer(sp, 10);
        // component.PauseDragOverride = false; PreventGripping = true: dragging and gripping (not rewritten).
        self.v45.crossbows[c].hasSyringe = true;
    }

    /// Utils.SetLayer(gameObject, layer) on a physical's object (children tagged RecursiveLayerChild: none on a syringe).
    fn v45_set_layer(&mut self, p: usize, layer: i32) {
        let id = self.physicals[p].id;
        self.with_component_instance(id, |g, inst, o| g.game_object_set_layer(inst, o, layer));
    }

    /// CrossbowBehaviour.DisconnectSyringe
    fn crossbow_disconnect_syringe(&mut self, c: usize) {
        if let Some(j) = self.v45.crossbows[c].syringeJoint.take() {
            self.fixed_joint_destroy(j);
        }
        if let Some(s) = self.v45.crossbows[c].AttachedSyringe {
            let sp = self.syringes[s].Phys;
            // StartCoroutine(DelayCoroutine(1, SetLayer(9))).
            self.v45.delayed_layers.push((self.time.time + 1.0, sp));
            if !self.physicals[sp].destroyed {
                // PreventGripping = false; PauseDragOverride = true (not rewritten).
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
    }

    /// CrossbowBehaviour.Update
    fn crossbow_update(&mut self, c: usize) {
        if let Some(s) = self.v45.crossbows[c].AttachedSyringe.filter(|&s| !self.physicals[self.syringes[s].Phys].destroyed) {
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
        // The crossbow's coroutines step after its Update (Unity runs them after every Update).
        self.v45_coroutines();
    }

    /// The delayed SetLayer(9) of disconnected syringes (Utils.DelayCoroutine(1f, ...)), run with the coroutines.
    fn v45_coroutines(&mut self) {
        let now = self.time.time;
        let due: Vec<usize> = self.v45.delayed_layers.iter().filter(|&&(t, _)| now >= t).map(|&(_, p)| p).collect();
        self.v45.delayed_layers.retain(|&(t, _)| now < t);
        for p in due {
            if !self.physicals[p].destroyed {
                self.v45_set_layer(p, 9);
            }
        }
    }

    /// CrossbowBehaviour.OnLaunch (Launcher.OnLaunch): the launched bolt gets an InjectableBehaviour holding
    /// SyringeFluidUsage of each liquid the attached syringe holds.
    pub(super) fn crossbow_on_launch(&mut self, l: usize, bolt: Option<usize>) {
        if !crate::assets::il::is_1_28_45() {
            return;
        }
        for c in 0..self.v45.crossbows.len() {
            if self.v45.crossbows[c].Launcher != l {
                continue;
            }
            let Some(s) = self.v45.crossbows[c].AttachedSyringe.filter(|&s| !self.physicals[self.syringes[s].Phys].destroyed) else { continue };
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
                out.push((liquid, amount));
            }
            // Start: GetComponentInChildren<ParticleSystem>() gets FxColor and Play (rendering).
            self.v45.injectables.push(Injectable { id, phys: bolt, LiquidDistributions: Some(out), FxColor: fx, destroyed: false });
        }
    }

    /// InjectableBehaviour.Lodged (SendMessage("Lodged") to the stabber's GameObject).
    pub(super) fn injectable_lodged_on(&mut self, p: usize, victim: usize) {
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
            // Liquid.TryGetLiquid(LiquidID): the ids written by OnLaunch always resolve.
            for (liquid, amount) in self.v45.injectables[i].LiquidDistributions.take().unwrap() {
                self.add_liquid(target, liquid, amount);
            }
            // ImbueParticles.Stop() (rendering); Object.Destroy(this).
            self.v45.injectables[i].destroyed = true;
        }
    }

    // ---- BuzzerBehaviour -----------------------------------------------------------------------------------------

    /// BuzzerBehaviour.Start: the "chooseAudioClip" (FilePicker) and "setLoudness" (float dialog) buttons are UI; a
    /// saved AudioData is decoded into the clip; then UpdateLoudness and UpdateColour.
    fn buzzer_start(&mut self, b: usize) {
        if !self.v45.buzzers[b].AudioData.trim().is_empty() {
            unimplemented!("BuzzerBehaviour.Start: a saved AudioData (Utils.FBSF and ClipFromData's NAudio conversion are not rewritten)");
        }
        self.buzzer_update_loudness(b);
        // UpdateColour: CurrentDataId 0 keeps SpriteRenderer.color white (rendering).
    }

    /// BuzzerBehaviour.UpdateLoudness
    fn buzzer_update_loudness(&mut self, b: usize) {
        let (num, num2) = (1.2136364f32, 24.272728f32);
        let x = &mut self.v45.buzzers[b];
        let num3 = mathf::clamp01(mathf::map_range(num, num2, 0.0, 1.0, x.Loudness));
        x.min_distance = mathf::lerp(num, num2, num3);
        x.max_distance = mathf::lerp(30.0, 100.0, num3);
    }

    /// BuzzerBehaviour.Use: Source.PlayOneShot(clip ?? FallbackAudioClip) at most every 0.1 s, unless destroyed.
    fn buzzer_use(&mut self, b: usize) {
        let x = &self.v45.buzzers[b];
        let destroyed = x.DamagableBehaviour.map_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"), |m| self.machinery[m].Destroyed);
        if x.enabled && !destroyed && !(self.time.time - x.lastPlayedTime < 0.1) {
            let id = x.id;
            let clip = self.audio.clip(id, "FallbackAudioClip");
            if let Some(source) = self.audio.source_field(id, "Source") {
                self.audio.play_one_shot(source, clip.as_ref(), 1.0);
            }
            self.v45.buzzers[b].lastPlayedTime = self.time.time;
        }
    }

    // ---- DulcamarusBehaviour -------------------------------------------------------------------------------------

    /// Whether the person's GameObject has a DulcamarusBehaviour (HasComponent).
    pub(super) fn dulcamarus_on(&self, person: usize) -> bool {
        self.v45.dulcamarus.iter().any(|d| d.Person == person && !d.destroyed)
    }

    /// gameObject.AddComponent<DulcamarusBehaviour>() on a person: Awake now, Update from the next frame, Start pending.
    pub(super) fn dulcamarus_add(&mut self, person: usize) -> usize {
        let id = self.allocate_instance_id();
        // Awake: head = Person.Limbs.FirstOrDefault(HasBrain); new GameObject("siphoning tendrils", typeof(Optout))
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
        let b = Behaviour::V45(V::Dulcamarus(d));
        self.behaviour_of.insert(id, (0, b));
        self.behaviours.add(0, b);
        self.pending_starts.push(super::misc::PendingStart::V45(V::Dulcamarus(d)));
        d
    }

    /// DulcamarusBehaviour.Start: a DivineGlow at each connected limb, by a coin flip each.
    fn dulcamarus_start(&mut self, d: usize) {
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

    fn limb_position(&self, l: usize) -> [f32; 3] {
        self.physicals[self.limbs[l].PhysicalBehaviour].transform.position
    }

    fn limb_alive(&self, l: usize) -> bool {
        !self.physicals[self.limbs[l].PhysicalBehaviour].destroyed
    }

    /// DulcamarusBehaviour.Update
    fn dulcamarus_update(&mut self, d: usize) {
        let head = self.v45.dulcamarus[d].head.filter(|&h| self.limb_alive(h));
        if head.is_none_or(|h| self.physicals[self.limbs[h].PhysicalBehaviour].isDisintegrated || self.limbs[h].IsZombie) {
            let x = &mut self.v45.dulcamarus[d];
            x.siphonSources = Default::default();
            x.indexByLimb.clear();
            // The tendrils are destroyed (their GameObjects).
            x.shouldHeal = false;
        }
        let Some(head) = head else { super::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        let time = self.time.time;
        let dt = self.time.deltaTime;
        let position = self.limb_position(head);
        for i in 0..16 {
            let Some(mut s) = self.v45.dulcamarus[d].siphonSources[i].take() else { continue };
            if let Some(line) = s.LineRenderer.as_mut() {
                let seed_frac = s.Seed - s.Seed.floor();
                let num = mathf::clamp01(s.Strength) * mathf::clamp01(s.Time * mathf::lerp(1.0, 0.5, seed_frac));
                let num2 = num * num;
                line.start_color = [num, num, num, 1.0];
                line.end_color = [num2, num2, num2, 1.0];
                let num3 = mathf::map_range(0.0, 1.0, 1.0, 4.0, (s.Seed / 2.0).floor());
                let sa = |past: [f32; 3], target: [f32; 3], speed: f32| [super::fire::smooth_approach(past[0], target[0], speed, dt), super::fire::smooth_approach(past[1], target[1], speed, dt), 0.0];
                s.SourcePos = sa(s.SourcePos, position, num3 * 2.0);
                if self.limb_alive(s.Limb) {
                    s.TargetPos = sa(s.TargetPos, self.limb_position(s.Limb), num3);
                }
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
            if s.Strength <= 0.0 {
                // The LineRenderer's GameObject is destroyed; indexByLimb.Remove(Limb).
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
                } else if let Some(value) = self.v45.dulcamarus[d].siphonSources.iter().position(Option::is_none) {
                    self.v45.dulcamarus[d].indexByLimb.insert(l, value);
                    // new SiphonSource { ... }: the Seed initialiser draws first, then the LineRenderer's coin flip.
                    let seed = self.random.value() * 1000.0;
                    let target = self.limb_position(l);
                    let line = if self.random.value() > 0.5 { Some(self.dulcamarus_create_line_renderer()) } else { None };
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
    fn dulcamarus_create_line_renderer(&mut self) -> Tendril {
        self.allocate_instance_id();
        self.allocate_instance_id();
        let id = self.allocate_instance_id();
        let width = self.random.range(0.05, 0.4);
        Tendril { id, width, start_color: [0.0, 0.0, 0.0, 1.0], end_color: [1.0; 4], positions: vec![[0.0; 3]; 12], enabled: false }
    }

    /// DulcamarusBehaviour.Heal
    fn dulcamarus_heal(&mut self, d: usize) {
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
    fn dripping_blood_update(&mut self, d: usize) {
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
                // Rigidbody2D.GetRelativePoint (SlashColliders: none in the rewrite, so no ClosestPoint).
                self.sync_pending();
                let xf = self.world.body(self.physicals[p].rigidbody.body).xf;
                let at = [xf.q.c * local[0] - xf.q.s * local[1] + xf.p.x, xf.q.s * local[0] + xf.q.c * local[1] + xf.p.y];
                let colour = self.v45.drips[d].Colour;
                self.v45.drips[d].emitted.push((at, colour));
            }
        }
    }

    /// PhysicalBehaviour.Start: Instantiate(Global.main.DrippingParticlesPrefab, transform, false), localPosition = 0,
    /// GetComponent<DrippingBloodParticleController>().PhysicalBehaviour = this. The clone's ids and its Update.
    pub(super) fn dripping_blood_instantiate(&mut self, p: usize) -> usize {
        if self.v45.drip_prefab.is_none() {
            let source = self.global_prefab("DrippingParticlesPrefab");
            let db = self.assets.as_mut().expect("the game's data");
            let prefab = db.prefab(source).expect("Global.DrippingParticlesPrefab");
            let mut at = None;
            for (o, obj) in prefab.objects.iter().enumerate() {
                for (k, (_, c)) in obj.components.iter().enumerate() {
                    match c {
                        Component::Script { class, fields, .. } if class == "DrippingBloodParticleController" => at = Some((o, k, f(fields, "Amount", 1.0), f(fields, "DecaySpeed", 0.25))),
                        // Optout is a marker (no messages).
                        Component::Script { class, .. } if class == "Optout" => {}
                        Component::Script { class, .. } => unimplemented!("mod-only: a {class} on Global.DrippingParticlesPrefab"),
                        _ => {}
                    }
                }
            }
            let (o, k, amount, decay) = at.expect("DrippingParticlesPrefab's DrippingBloodParticleController");
            let objects = prefab.objects.iter().map(|x| (1 + x.components.len(), x.parent)).collect();
            self.v45.drip_prefab = Some((objects, (o, k), amount, decay));
        }
        let (objects, (o, k), amount, decay) = self.v45.drip_prefab.clone().unwrap();
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
        let b = Behaviour::V45(V::DrippingBlood(d));
        self.behaviour_of.insert(controller, (0, b));
        self.behaviours.add(0, b);
        d
    }

    /// PhysicalBehaviour.FixedUpdate's slash drip: LocalDripAreas.Add(point), Colour = colour, Amount = amount.
    pub(super) fn dripping_blood_drip(&mut self, d: usize, local: [f32; 2], colour: [f32; 4], amount: f32) {
        let x = &mut self.v45.drips[d];
        x.LocalDripAreas.add(local);
        x.Colour = colour;
        x.Amount = amount;
    }

    /// DrippingBloodParticleController.Set
    pub fn dripping_blood_set(&mut self, d: usize, color: [f32; 4], amount: f32) {
        if !(amount <= f32::EPSILON) {
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
        frames(&mut g, 30);
        let s = &g.v45.speedometers[0];
        assert!(s.speedMs > 1.0 && s.TopSpeedMs >= s.speedMs, "{} {}", s.speedMs, s.TopSpeedMs);
        g.speedometer_set_text(0);
        assert!(g.v45.speedometers[0].text.contains(" m/s\n<color=blue>"), "{}", g.v45.speedometers[0].text);
        assert!(g.physicals[p].menu.iter().any(|b| b.Identity == "resetTopSpeed"));
        g.activate_for_test(p);
        frames(&mut g, 1);
        assert!(!g.v45.speedometers[0].Activated);
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
        g.activate_for_test(p);
        frames(&mut g, 1);
        assert!(g.v45.buzzers[0].lastPlayedTime > 0.0);
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
