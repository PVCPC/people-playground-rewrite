//! An instantiated prefab's MonoBehaviours as the Game's component arenas. Each script gets its arena entry with
//! its serialized fields, and the rewrite's links (arena indices) stand for its component references: the
//! serialized ones resolve through the prefab, the GetComponent ones through the object's components. Runtime
//! state stays at its field initializer for Awake/OnEnable/Start to set.

use super::circulation::Circulation;
use super::limb::{GoreString, Limb};
use super::misc::{AutofixJoint, AutofixJoints, GForce, Grip, Node, Skin};
use super::person::{LimbPose, Person, RagdollPose};
use super::physical::{Physical, PhysicalProperties, SharpAxis};
use super::water::Water;
use super::spawn::{anchored_initial_state, joint_connected_object, load_values, Instance};
use super::*;
use crate::assets::classes::PPtr;
use crate::assets::components::{JointKind, PhysicsMaterial2D, Sprite};
use crate::assets::db::{AssetDb, Component, ObjRef, Prefab, PrefabObject};

/// Where the modelled managed heap starts (see `Game::spawn`): a 2048-aligned address. Any value is as real as another
/// (the game's own differs per run); this one is a fixed pick.
pub(super) const MODELLED_HEAP_BASE: u64 = 0x0000_0000_014f_5000;
use crate::assets::mono::Value;
use crate::game_assets::{Curve, CurveCache};
use crate::unity::transform::Node as TransformNode;
use std::collections::HashMap;

/// The arena index of each prefab object's scripts, in prefab (GetComponentsInChildren) order.
#[derive(Default)]
struct Arenas {
    physical: HashMap<usize, usize>,
    limb: HashMap<usize, usize>,
    grip: HashMap<usize, usize>,
    hinge: HashMap<(usize, usize), usize>,
    /// Transform nodes by object (the Rigidbody-less parents of bodies).
    node: HashMap<usize, usize>,
}

/// The arena entries `link_instance_reusing` keeps: physical, limb and grip index by object, hinge index by (object, component).
#[derive(Default)]
pub struct Reuse {
    pub physical: HashMap<usize, usize>,
    pub limb: HashMap<usize, usize>,
    pub grip: HashMap<usize, usize>,
    pub hinge: HashMap<(usize, usize), usize>,
    /// The syringes a recording's loader built: they keep their state.
    pub syringe: HashMap<usize, usize>,
}

/// A reused (recorded) PhysicalBehaviour takes what only the prefab gives from the linked one `p`: the record keeps its state.
fn fill_from_prefab(q: &mut Physical, p: Physical) {
    q.SoundVolumeBoost = p.SoundVolumeBoost;
    q.Resizable = p.Resizable;
    q.ReflectsLasers = p.ReflectsLasers;
    q.AbsorbsLasers = p.AbsorbsLasers;
    q.audio_time_scale = p.audio_time_scale;
    q.prefab_decal_controllers = p.prefab_decal_controllers;
    q.ActivationPropagationDelay = p.ActivationPropagationDelay;
    q.SendUserPropagation = p.SendUserPropagation;
    q.BulletPenetration = p.BulletPenetration;
    q.BulletDeflectChanceOverride = p.BulletDeflectChanceOverride;
    q.OverrideShotSounds = p.OverrideShotSounds;
    q.collider_defs = p.collider_defs;
    // An inactive object's colliders have no recorded fixtures: the prefab's ids.
    if q.all_colliders.is_empty() {
        (q.colliders, q.all_colliders) = (p.colliders, p.all_colliders);
    }
    q.has_optout = p.has_optout;
    q.menu = p.menu;
    q.driven_by = p.driven_by;
    q.rigidbody.constraints = p.rigidbody.constraints;
    q.rigidbody.detached_props = p.rigidbody.detached_props;
    q.sprite = p.sprite;
    q.sprite_enabled = p.sprite_enabled;
    q.sprite_id = p.sprite_id;
    q.sprite_layer = p.sprite_layer;
    q.sprite_game_layer = p.sprite_game_layer;
    q.sprite_material = p.sprite_material;
    q.sprite_draw = p.sprite_draw;
    q.sprite_bounds = q.sprite_bounds.or(p.sprite_bounds);
}

/// What `link_instance` built: the Person's index (a root PersonBehaviour), and the arena index of each limb,
/// PhysicalBehaviour and MapLightBehaviour object and of each Rigidbody-less Transform above a body.
#[derive(Clone)]
pub struct Linked {
    /// The visual scripts (visual.rs) by (object, component).
    pub visuals: HashMap<(usize, usize), usize>,
    pub person: Option<usize>,
    pub limbs: HashMap<usize, usize>,
    pub nodes: HashMap<usize, usize>,
    pub physicals: HashMap<usize, usize>,
    /// Game::hinges index by (object, component).
    pub hinges: HashMap<(usize, usize), usize>,
    pub map_lights: HashMap<usize, usize>,
    /// Game::waters index by object.
    pub waters: HashMap<usize, usize>,
    pub machinery: HashMap<usize, usize>,
    /// Game::lavas, spawners and heat_rects index by object.
    pub lavas: HashMap<usize, usize>,
    pub destroyables: HashMap<usize, usize>,
    pub fire_maps: HashMap<usize, usize>,
    pub firearms: HashMap<usize, usize>,
    pub launchers: HashMap<usize, usize>,
    pub bells: HashMap<usize, usize>,
    pub machine_guns: HashMap<usize, usize>,
    pub miniguns: HashMap<usize, usize>,
    pub accelerators: HashMap<usize, usize>,
    pub accelerator_bolts: HashMap<usize, usize>,
    pub archelix_casters: HashMap<usize, usize>,
    pub archelix_bolts: HashMap<usize, usize>,
    pub universal_bells: HashMap<usize, usize>,
    pub counters: HashMap<usize, usize>,
    pub pointers: HashMap<usize, usize>,
    pub toggleable_mirrors: HashMap<usize, usize>,
    pub valves: HashMap<usize, usize>,
    pub liquid_canisters: HashMap<usize, usize>,
    pub liquid_filters: HashMap<usize, usize>,
    pub pulse_drums: HashMap<usize, usize>,
    pub liquidentifiers: HashMap<usize, usize>,
    pub centrifuges: HashMap<usize, usize>,
    pub vibrations: HashMap<usize, usize>,
    pub liquid_pumps: HashMap<usize, usize>,
    pub liquid_duplicators: HashMap<usize, usize>,
    pub liquid_holes: HashMap<usize, usize>,
    pub timed_gates: HashMap<usize, usize>,
    pub lagboxes: HashMap<usize, usize>,
    pub gates: HashMap<usize, usize>,
    pub activation_transformers: HashMap<usize, usize>,
    pub hover_thrusters: HashMap<usize, usize>,
    pub hovercars: HashMap<usize, usize>,
    pub winches: HashMap<usize, usize>,
    pub conveyors: HashMap<usize, usize>,
    pub reactor_cores: HashMap<usize, usize>,
    pub reactor_conduits: HashMap<usize, usize>,
    pub reactor_conduit_sets: HashMap<usize, usize>,
    pub reactor_a5_displays: HashMap<usize, usize>,
    pub control_replacers: HashMap<usize, usize>,
    pub reactor_meltdowns: HashMap<usize, usize>,
    pub do_at_times: HashMap<usize, usize>,
    pub industrial_lifts: HashMap<usize, usize>,
    pub copper_wires: HashMap<usize, usize>,
    pub eps: HashMap<usize, usize>,
    pub acid_pools: HashMap<usize, usize>,
    pub vacuums: HashMap<usize, usize>,
    pub point_ats: HashMap<usize, usize>,
    pub random_sounds: HashMap<usize, usize>,
    pub loop_sounds: HashMap<usize, usize>,
    pub joint_motor_sounds: HashMap<usize, usize>,
    pub play_random_clips: HashMap<usize, usize>,
    pub jukeboxes: HashMap<usize, usize>,
    pub antenna_receivers: HashMap<usize, usize>,
    pub couplers: HashMap<usize, usize>,
    pub lightning_entities: HashMap<usize, usize>,
    pub pyro_entities: HashMap<usize, usize>,
    pub seismic_explosions: HashMap<usize, usize>,
    pub energy_bolts: HashMap<usize, usize>,
    pub transmitters: HashMap<usize, usize>,
    pub defibrillators: HashMap<usize, usize>,
    pub cardiopulmonary_bypasses: HashMap<usize, usize>,
    pub heart_monitors: HashMap<usize, usize>,
    pub excaliburs: HashMap<usize, usize>,
    pub sharp_on_all_sides: HashMap<usize, usize>,
    pub blood_tanks: HashMap<usize, usize>,
    pub liquid_outlets: HashMap<usize, usize>,
    pub explosives: HashMap<usize, usize>,
    pub delete_afters: HashMap<usize, usize>,
    pub point_to_velocities: HashMap<usize, usize>,
    pub inverters: HashMap<usize, usize>,
    pub obstruction_emitters: HashMap<usize, usize>,
    pub android_sparks: HashMap<usize, usize>,
    pub cars: HashMap<usize, usize>,
    pub tyres: HashMap<usize, usize>,
    pub flasks: HashMap<usize, usize>,
    pub liquid_containers: HashMap<usize, usize>,
    pub truck_doors: HashMap<usize, usize>,
    pub glowtubes: HashMap<usize, usize>,
    pub cathode_lights: HashMap<usize, usize>,
    pub leds: HashMap<usize, usize>,
    pub detectors: HashMap<usize, usize>,
    pub sticky_grenades: HashMap<usize, usize>,
    pub generators: HashMap<usize, usize>,
    pub floodlights: HashMap<usize, usize>,
    pub spike_grenades: HashMap<usize, usize>,
    pub heating_elements: HashMap<usize, usize>,
    pub ray_guns: HashMap<usize, usize>,
    pub bulbs: HashMap<usize, usize>,
    pub thrusterbeds: HashMap<usize, usize>,
    pub pistons: HashMap<usize, usize>,
    pub map_lifts: HashMap<usize, usize>,
    pub flamethrowers: HashMap<usize, usize>,
    pub rockets: HashMap<usize, usize>,
    pub airfoils: HashMap<usize, usize>,
    pub stunners: HashMap<usize, usize>,
    pub soaps: HashMap<usize, usize>,
    pub power_hammers: HashMap<usize, usize>,
    pub balloons: HashMap<usize, usize>,
    pub metronomes: HashMap<usize, usize>,
    pub determined_anomalies: HashMap<usize, usize>,
    pub random_sprite_children: HashMap<usize, usize>,
    pub physics_guns: HashMap<usize, usize>,
    pub activator_electrodes: HashMap<usize, usize>,
    pub thermometers: HashMap<usize, usize>,
    pub infrared_thermometers: HashMap<usize, usize>,
    pub activation_toggles: HashMap<usize, usize>,
    pub power_gauges: HashMap<usize, usize>,
    pub torches: HashMap<usize, usize>,
    pub flares: HashMap<usize, usize>,
    pub buttons: HashMap<usize, usize>,
    pub lasers: HashMap<usize, usize>,
    pub firearm_attachments: HashMap<usize, usize>,
    pub televisions: HashMap<usize, usize>,
    pub displays: HashMap<usize, usize>,
    pub syringes: HashMap<usize, usize>,
    pub jet_engines: HashMap<usize, usize>,
    pub chainsaws: HashMap<usize, usize>,
    pub freezes: HashMap<usize, usize>,
    pub freeze_serialisations: HashMap<usize, usize>,
    pub ceiling_turrets: HashMap<usize, usize>,
    pub temperature_transfers: HashMap<usize, usize>,
    pub worm_staffs: HashMap<usize, usize>,
    pub atom_explosions: HashMap<usize, usize>,
    pub prefab_bombs: HashMap<usize, usize>,
    pub laser_turrets: HashMap<usize, usize>,
    pub sentry_turrets: HashMap<usize, usize>,
    pub sentry_guns: HashMap<usize, usize>,
    pub ion_bolts: HashMap<usize, usize>,
    pub random_events: HashMap<usize, usize>,
    pub emps: HashMap<usize, usize>,
    pub decimators: HashMap<usize, usize>,
    pub drones: HashMap<usize, usize>,
    pub portals: HashMap<usize, usize>,
    pub black_holes: HashMap<usize, usize>,
    pub immobility_fields: HashMap<usize, usize>,
    pub lightning_guns: HashMap<usize, usize>,
    pub javelins: HashMap<usize, usize>,
    pub javelin_missiles: HashMap<usize, usize>,
    pub tesla_coils: HashMap<usize, usize>,
    /// What each coil's bolt prefab gives its clones, by the coil's object.
    pub tesla_bolt_sources: HashMap<usize, super::tesla_coil::BoltSource>,
    pub energy_swords: HashMap<usize, usize>,
    pub blasters: HashMap<usize, usize>,
    pub beam_cannons: HashMap<usize, usize>,
    pub beam_repeaters: HashMap<usize, usize>,
    pub android_lasers: HashMap<usize, usize>,
    pub ray_cannons: HashMap<usize, usize>,
    pub railguns: HashMap<usize, usize>,
    pub laser_receivers: HashMap<usize, usize>,
    pub key_triggers: HashMap<usize, usize>,
    pub liquid_level_gates: HashMap<usize, usize>,
    pub fireworks: HashMap<usize, usize>,
    pub blasterbolts: HashMap<usize, usize>,
    pub beamformers: HashMap<usize, usize>,
    pub scifi40s: HashMap<usize, usize>,
    pub effect_guns: HashMap<usize, usize>,
    pub signal_converters: HashMap<usize, usize>,
    pub syringe_explosions: HashMap<usize, usize>,
    pub point_liquids: HashMap<usize, usize>,
    pub act_on_collides: HashMap<usize, usize>,
    pub spawners: HashMap<usize, usize>,
    pub heat_rects: HashMap<usize, usize>,
    pub random_sprites: HashMap<usize, usize>,
    /// Game::node_rigidbodies index by object.
    pub node_rigidbodies: HashMap<usize, usize>,
    /// The PlayAudioAtRandom on the instance's parent (`ambient_audio`), which MapConfig.Reinitialise configures.
    pub parent_audio: Option<usize>,
    /// The translated components (tools/translator) by (object, component index).
    pub translated: HashMap<(usize, usize), super::translated::TBehaviour>,
}

/// The clone's Transform as a physical object's.
pub(super) fn pose(inst: &Instance, o: usize) -> Pose {
    let trs = inst.objects[o].trs;
    Pose { position: inst.position(o), rotation: inst.rotation(o), localScale: [trs.s[0], trs.s[1], trs.s[2]], local: Some(([trs.t[0], trs.t[1], trs.t[2]], trs.q)) }
}

/// HoverPoints[i].localPosition: the points must be plain children (no physics) of the script's object `o`.
fn hover_points(prefab: &Prefab, inst: &super::spawn::Instance, file: usize, o: usize, v: &Value, class: &str) -> Result<Vec<[f32; 3]>, String> {
    let mut points = Vec::new();
    for r in array(v, "HoverPoints")? {
        let r = r.pptr().ok_or_else(|| format!("{class}.HoverPoints"))?;
        let (ho, _) = *prefab.index.get(&ObjRef { file, path_id: r.path_id }).filter(|_| r.file == 0 && !r.is_null()).ok_or_else(|| format!("{class}.HoverPoints outside the prefab"))?;
        if inst.objects[ho].parent != Some(o) || prefab.objects[ho].components.iter().any(|(_, k)| matches!(k, Component::Rigidbody2D(_) | Component::Collider2D(_) | Component::Joint2D(_))) {
            unimplemented!("mod-only: {class}.HoverPoints: a point that is not a plain child of the script's object");
        }
        let t = inst.objects[ho].trs.t;
        points.push([t[0], t[1], t[2]]);
    }
    Ok(points)
}

fn script<'a>(prefab: &'a Prefab, o: usize, class: &str) -> Option<(usize, bool, &'a Value)> {
    prefab.objects[o].components.iter().enumerate().find_map(|(c, (_, comp))| match comp {
        Component::Script { class: k, enabled, fields, .. } if k == class => Some((c, *enabled, fields)),
        _ => None,
    })
}

/// Every `class` script on object `o`, in component order.
fn scripts<'a>(prefab: &'a Prefab, o: usize, class: &'a str) -> impl Iterator<Item = (usize, bool, &'a Value)> + 'a {
    prefab.objects[o].components.iter().enumerate().filter_map(move |(c, (_, comp))| match comp {
        Component::Script { class: k, enabled, fields, .. } if k == class => Some((c, *enabled, fields)),
        _ => None,
    })
}

fn field<'a>(v: &'a Value, name: &str) -> Result<&'a Value, String> {
    v.get(name).ok_or_else(|| format!("no serialized {name}"))
}

fn float(v: &Value, name: &str) -> Result<f32, String> {
    field(v, name)?.f32().ok_or_else(|| format!("{name}: not a float"))
}

fn array<'a>(v: &'a Value, name: &str) -> Result<&'a [Value], String> {
    field(v, name)?.array().ok_or_else(|| format!("{name}: not an array"))
}

fn string(v: &Value, name: &str) -> Result<String, String> {
    match field(v, name)? {
        Value::Str(s) => Ok(s.clone()),
        x => Err(format!("{name}: not a string: {x:?}")),
    }
}

fn floats<const N: usize>(v: &Value) -> Result<[f32; N], String> {
    let mut out = [0.0; N];
    out.set(v)?;
    Ok(out)
}

/// A serialized ExplosionCreator.ExplosionParameters.
fn explosion_parameters(v: &Value) -> Result<super::explosion::ExplosionParameters, String> {
    use super::explosion::EffectSize;
    let int = |name: &str| field(v, name)?.i64().ok_or_else(|| format!("{name}: not an integer"));
    Ok(super::explosion::ExplosionParameters {
        FragmentationRayCount: int("FragmentationRayCount")? as u32,
        Position: floats::<3>(field(v, "Position")?)?,
        Range: float(v, "Range")?,
        FragmentForce: float(v, "FragmentForce")?,
        CreateParticlesAndSound: field(v, "CreateParticlesAndSound")?.bool().ok_or("CreateParticlesAndSound")?,
        EffectSize: match int("EffectSize")? {
            0 => EffectSize::Medium,
            1 => EffectSize::Small,
            2 => EffectSize::Large,
            x => return Err(format!("EffectSize {x}")),
        },
        DismemberChance: float(v, "DismemberChance")?,
        BallisticShrapnelCount: int("BallisticShrapnelCount")? as i32,
    })
}

/// A serialized Bounds as center then extents.
fn bounds(v: &Value) -> Result<[f32; 6], String> {
    let [cx, cy, cz] = floats::<3>(field(v, "m_Center")?)?;
    let [ex, ey, ez] = floats::<3>(field(v, "m_Extent")?)?;
    Ok([cx, cy, cz, ex, ey, ez])
}

/// A LineSegment (A, B).
fn line(v: &Value) -> Result<[f32; 4], String> {
    let [ax, ay] = floats::<2>(field(v, "A")?)?;
    let [bx, by] = floats::<2>(field(v, "B")?)?;
    Ok([ax, ay, bx, by])
}

pub(crate) fn curve(v: &Value) -> Result<Curve, String> {
    let mut keys = Vec::new();
    for k in array(v, "m_Curve")? {
        if field(k, "weightedMode")?.i64() != Some(0) {
            unimplemented!("mod-only: a weighted AnimationCurve key");
        }
        keys.push([float(k, "time")?, float(k, "value")?, float(k, "inSlope")?, float(k, "outSlope")?]);
    }
    let int = |n: &str| -> Result<i32, String> { Ok(field(v, n)?.i64().ok_or(n.to_string())? as i32) };
    Ok(Curve { keys, pre: int("m_PreInfinity")?, post: int("m_PostInfinity")? })
}

/// Sprite.bounds (Sprite::GetBounds): the sprite rect around its pivot in units, 0.1 deep either side.
/// A UnityEngine.Bounds field as the 2D box the map scripts use.
pub(super) fn map_bounds(v: &Value) -> Result<Bounds, String> {
    let b = bounds(field(v, "BoundingBox")?)?;
    Ok(Bounds { center: [b[0], b[1]], extents: [b[3], b[4]] })
}

pub(super) fn sprite_bounds(s: &Sprite) -> ([f32; 3], [f32; 3]) {
    let (w, h) = (s.rect[2] / s.pixels_to_units, s.rect[3] / s.pixels_to_units);
    let (ex, ey) = (w * 0.5, h * 0.5);
    ([ex - s.pivot[0] * w, ey - s.pivot[1] * h, 0.0], [ex, ey, 0.1])
}

/// SpriteRenderer::GetSpriteBounds (1806e6840): Simple takes the sprite's bounds, mirrored by the flips
/// (Sprite::GetBounds negates the corners); Sliced and Tiled the renderer's size around the pivot, the flips as
/// signs, encapsulated from an empty AABB with z -0.1 and 0.1.
/// The LimbStatus prefab's look (LimbStatusBehaviour on the root, its Bar child's SpriteRenderer).
fn limb_status_look(db: &mut AssetDb, file: usize, reference: &Value) -> Result<Option<super::limb::LimbStatusLook>, String> {
    let Some(root) = reference.pptr().and_then(|p| db.resolve(file, p)) else { return Ok(None) };
    let prefab = db.prefab(root)?;
    let at = root.file;
    let renderer = |o: usize| prefab.objects[o].components.iter().find_map(|(_, c)| match c {
        Component::SpriteRenderer(r) => Some(r.clone()),
        _ => None,
    }).ok_or("LimbStatus: a SpriteRenderer is missing");
    let (_, _, v) = script(&prefab, 0, "LimbStatusBehaviour").ok_or("LimbStatus without LimbStatusBehaviour")?;
    let sprite = |db: &AssetDb, name: &str| -> Result<crate::assets::db::ObjRef, String> { db.resolve(at, field(v, name)?.pptr().ok_or("sprite")?).ok_or_else(|| format!("LimbStatus: {name} is null")) };
    let part = |r: &crate::assets::components::SpriteRenderer, s| (s, r.color, r.sorting_order, r.sorting_layer_id, ([r.flip_x, r.flip_y], r.draw_mode, r.size, r.adaptive_mode_threshold, r.sprite_tile_mode == 1));
    let root_r = renderer(0)?;
    let bar = (1..prefab.objects.len()).find(|&o| prefab.objects[o].name == "bar").ok_or("LimbStatus without its bar")?;
    let bar_r = renderer(bar)?;
    Ok(Some(super::limb::LimbStatusLook {
        frame: part(&root_r, sprite(db, "BarSprite")?),
        dead: part(&root_r, sprite(db, "DeadSprite")?),
        bar: part(&bar_r, db.resolve(at, bar_r.sprite).ok_or("LimbStatus bar without a sprite")?),
        BarMaxWidth: field(v, "BarMaxWidth")?.f32().ok_or("BarMaxWidth")?,
    }))
}

pub(crate) fn renderer_bounds(s: &Sprite, r: &crate::assets::components::SpriteRenderer) -> ([f32; 3], [f32; 3]) {
    if r.draw_mode == 0 {
        let (mut c, e) = sprite_bounds(s);
        if r.flip_x {
            c[0] = -c[0];
        }
        if r.flip_y {
            c[1] = -c[1];
        }
        return (c, e);
    }
    let sign = |flip: bool| if flip { -1.0f32 } else { 1.0 };
    let corner = |size: f32, pivot: f32, f: f32| {
        let below = size * pivot * f;
        (size * f - below, -below)
    };
    let (hx, lx) = corner(r.size[0], s.pivot[0], sign(r.flip_x));
    let (hy, ly) = corner(r.size[1], s.pivot[1], sign(r.flip_y));
    let (min, max) = ([hx.min(lx), hy.min(ly), -0.1f32], [hx.max(lx), hy.max(ly), 0.1f32]);
    (std::array::from_fn(|i| (min[i] + max[i]) * 0.5), std::array::from_fn(|i| (max[i] - min[i]) * 0.5))
}

struct Linker<'a> {
    prefab: &'a Prefab,
    inst: &'a Instance,
    file: usize,
    arenas: Arenas,
    /// Game::link_under: what the clone root hangs from (None: a root).
    under: Option<(BareAnchor, Vec<crate::unity::transform::Trs>)>,
}

impl Linker<'_> {
    /// Where object `o`'s Transform hangs: the nearest physical or node above it, and the local TRS of each object between (`o` first).
    fn anchor_chain(&self, o: usize) -> (BareAnchor, Vec<crate::unity::transform::Trs>) {
        let (mut chain, mut up) = (Vec::new(), Some(o));
        let anchor = loop {
            let Some(t) = up else {
                let Some((a, c)) = &self.under else { break BareAnchor::Root };
                chain.extend_from_slice(c);
                break *a;
            };
            if let Some(&p) = self.arenas.physical.get(&t) {
                break BareAnchor::Physical(p);
            }
            if let Some(&i) = self.arenas.node.get(&t) {
                break BareAnchor::Node(i);
            }
            chain.push(self.inst.objects[t].trs);
            up = self.inst.objects[t].parent;
        };
        (anchor, chain)
    }

    /// Object `o`'s Transform as a script's `base.transform`: its physical, else its anchor chain.
    fn object_xf(&self, o: usize) -> super::script_object::ObjectXf {
        match self.arenas.physical.get(&o) {
            Some(&p) => super::script_object::ObjectXf::Physical(p),
            None => {
                let (a, c) = self.anchor_chain(o);
                super::script_object::ObjectXf::Chain(a, c, self.inst.objects[0].components[0])
            }
        }
    }

    /// The LineRenderer (class 120) of the object `field` names, added to `Game::lines`.
    fn add_line(&self, game: &mut Game, db: &AssetDb, field: &Value, what: &str) -> Result<usize, String> {
        let o = self.object(field)?.ok_or_else(|| format!("{what}.LineRenderer is null"))?;
        self.add_line_on(game, db, o, what)
    }

    /// The LineRenderer (class 120) on object `o` itself (GetComponent<LineRenderer>()), added to `Game::lines`.
    fn add_line_on(&self, game: &mut Game, db: &AssetDb, o: usize, what: &str) -> Result<usize, String> {
        let (c, _) = self.prefab.objects[o].components.iter().enumerate().find(|(_, (_, k))| matches!(k, Component::Other(120))).map(|(c, x)| (c, x)).ok_or_else(|| format!("{what}.LineRenderer is no LineRenderer"))?;
        let data = crate::assets::components::LineRendererData::read(db.data(self.prefab.objects[o].components[c].0))?;
        let (anchor, chain) = self.anchor_chain(o);
        Ok(game.add_line(db, self.file, data, self.inst.objects[o].components[c], anchor, chain, self.prefab.objects[o].layer as i32, self.inst.objects[o].active))
    }

    /// The prefab object a local reference points at (its GameObject or one of its components).
    fn object(&self, v: &Value) -> Result<Option<usize>, String> {
        self.object_of(v.pptr().ok_or_else(|| format!("not a reference: {v:?}"))?)
    }

    fn object_of(&self, p: PPtr) -> Result<Option<usize>, String> {
        if p.is_null() {
            return Ok(None);
        }
        if p.file != 0 {
            return Err(format!("a reference outside the prefab: {p:?}"));
        }
        let (o, _) = self.prefab.index.get(&ObjRef { file: self.file, path_id: p.path_id }).ok_or_else(|| format!("dangling reference {p:?}"))?;
        Ok(Some(*o))
    }

    fn in_arena(&self, v: &Value, arena: &HashMap<usize, usize>, what: &str) -> Result<Option<usize>, String> {
        match self.object(v)? {
            None => Ok(None),
            Some(o) => arena.get(&o).copied().map(Some).ok_or_else(|| format!("{} has no {what}", self.prefab.objects[o].name)),
        }
    }

    fn limb(&self, v: &Value) -> Result<Option<usize>, String> {
        self.in_arena(v, &self.arenas.limb, "LimbBehaviour")
    }

    fn limbs(&self, v: &Value, name: &str) -> Result<Vec<usize>, String> {
        array(v, name)?.iter().map(|x| self.limb(x)?.ok_or_else(|| format!("{name}: a null entry"))).collect()
    }

    fn physical(&self, v: &Value) -> Result<Option<usize>, String> {
        self.in_arena(v, &self.arenas.physical, "PhysicalBehaviour")
    }
}

/// The names of an array of AudioClips, in order.
fn audio_clip_names(db: &crate::assets::db::AssetDb, file: usize, v: &Value, name: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for x in array(v, name)? {
        let r = db.resolve(file, x.pptr().ok_or(name)?).unwrap_or_else(|| unimplemented!("mod-only: a null AudioClip in {name}"));
        out.push(crate::assets::classes::AudioClip::read(db.data(r))?.name);
    }
    Ok(out)
}

/// A script's AudioSource field: the source's instance id (null or outside the prefab is mod-only: a NullReferenceException).
fn audio_source_field(prefab: &Prefab, inst: &Instance, file: usize, v: &Value, class: &str, name: &str) -> Result<i32, String> {
    let a = field(v, name)?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: {class}.{name} null or outside its prefab (NullReferenceException)"));
    let k = *prefab.index.get(&ObjRef { file, path_id: a.path_id }).ok_or_else(|| format!("{class}.{name}: dangling reference"))?;
    Ok(inst.objects[k.0].components[k.1])
}

/// An array of AudioClips, in order (None for a null element).
fn audio_clips(db: &crate::assets::db::AssetDb, cache: &mut std::collections::HashMap<ObjRef, super::audio::ClipRef>, file: usize, v: &Value, name: &str) -> Result<Vec<Option<super::audio::ClipRef>>, String> {
    array(v, name)?.iter().map(|x| super::audio::clip_ref(db, cache, db.resolve(file, x.pptr().ok_or(name)?))).collect()
}

/// The persistent calls of an event that target a CarBehaviour (SetHealth(h), set_enabled(false)), as the car's component
/// id and the call, in order.
fn car_calls(prefab: &Prefab, file: usize, event: &Value, inst: &Instance) -> Vec<(i32, super::car::CarCall)> {
    use super::car::CarCall;
    let mut out = Vec::new();
    for call in invoked_calls(event) {
        let Some((to, tc)) = call_target(prefab, file, call) else { continue };
        if !matches!(&prefab.objects[to].components[tc].1, Component::Script { class, .. } if class == "CarBehaviour") {
            continue;
        }
        let args = call.get("m_Arguments").unwrap();
        let method = string(call, "m_MethodName").unwrap();
        let call = match method.as_str() {
            "SetHealth" => CarCall::SetHealth(float(args, "m_FloatArgument").unwrap()),
            "set_enabled" if args.get("m_BoolArgument").is_some_and(|b| b.bool() == Some(false) || b.i64() == Some(0)) => CarCall::Disable,
            _ => CarCall::Stop(super::unity_event::stop_for(prefab, file, call, "a CarBehaviour call")),
        };
        out.push((inst.objects[to].components[tc], call));
    }
    out
}

/// A persistent call `event_calls` keeps: CarBehaviour's, DetachFromParent.Detach by its component id,
/// ActOnDestroy.Clear by its GameObject's Transform id, Behaviour.set_enabled(true) on a DeleteAfterTime (its component
/// id) or a RandomForceAtStart (its Rigidbody2D's component id, PositionalVelocityStrength, RotationalVelocityStrength),
/// or Rigidbody2D.set_simulated(true) (the Rigidbody2D's component id); InstantiateObjectBehaviour.Instantiate by its
/// component id; PhysicalBehaviour.HaltNextPropagation by its component id; Joint2D.set_breakForce(v) by the joint's
/// component id; Behaviour.set_enabled(v) on a UseEventTrigger by its component id; Rigidbody2D.WakeUp and
/// AddTorque(torque) by the Rigidbody2D's component id.
/// A UnityEvent persistent call on an AudioSource (by its component id), PhysicalBehaviour.PlayClipOnce (the physical's
/// component id) or AudioSourceToggleBehaviour.Toggle (the toggle's AudioSource field); clips by (file, PPtr) in
/// `Audio::event_clips`.
#[derive(Clone, Copy, Debug)]
pub enum AudioCall {
    Play,
    Stop,
    Pause,
    UnPause,
    PlayOneShot(usize, crate::assets::classes::PPtr),
    SetEnabled(bool),
    SetMute(bool),
    PlayClipOnce(usize, crate::assets::classes::PPtr),
    Toggle,
}

/// The audio call of a persistent call, if it is one (an AudioSource method this does not know panics).
fn audio_call(prefab: &Prefab, file: usize, inst: &Instance, call: &Value) -> Option<EventCall> {
    let (o, c) = call_target(prefab, file, call)?;
    let method = string(call, "m_MethodName").ok()?;
    let args = call.get("m_Arguments");
    let on = args.and_then(|a| a.get("m_BoolArgument")).and_then(|b| b.bool().or(b.i64().map(|i| i != 0)));
    let obj = args.and_then(|a| a.get("m_ObjectArgument")).and_then(Value::pptr).unwrap_or_default();
    let id = inst.objects[o].components[c];
    let a = match (&prefab.objects[o].components[c].1, method.as_str()) {
        (Component::AudioSource { .. }, "Play") => AudioCall::Play,
        (Component::AudioSource { .. }, "Stop") => AudioCall::Stop,
        (Component::AudioSource { .. }, "Pause") => AudioCall::Pause,
        (Component::AudioSource { .. }, "UnPause") => AudioCall::UnPause,
        (Component::AudioSource { .. }, "PlayOneShot") => AudioCall::PlayOneShot(file, obj),
        (Component::AudioSource { .. }, "set_enabled") if on.is_some() => AudioCall::SetEnabled(on.unwrap()),
        (Component::AudioSource { .. }, "set_mute") if on.is_some() => AudioCall::SetMute(on.unwrap()),
        (Component::AudioSource { .. }, _) => return Some(EventCall::Stop(super::unity_event::stop_for(prefab, file, call, "an AudioSource call"))),
        (Component::Script { class, .. }, "PlayClipOnce") if class == "PhysicalBehaviour" => AudioCall::PlayClipOnce(file, obj),
        (Component::Script { class, .. }, "Toggle") if class == "AudioSourceToggleBehaviour" => AudioCall::Toggle,
        _ => return None,
    };
    Some(EventCall::Audio(id, a))
}

#[derive(Clone, Copy, Debug)]
pub enum EventCall {
    /// An `AudioCall` on the component with this id.
    Audio(i32, AudioCall),
    Car(i32, super::car::CarCall),
    Detach(i32),
    ClearDestroyWith(i32),
    EnableDeleteAfter(i32),
    EnableRandomForce(i32, f32, f32),
    Simulate(i32),
    Instantiate(i32),
    AddRelativeForce(i32, f32),
    HaltNextPropagation(i32),
    SetBreakForce(i32, f32),
    SetUseEventEnabled(i32, bool),
    /// Behaviour.set_enabled(false) on a DamagableMachineryBehaviour.
    /// DamagableMachineryBehaviour.BreakPermanently by the machinery's component id.
    MachineryBreakPermanently(i32),
    DisableMachinery(i32),
    /// JetEngineBehaviour.ForceShutoff by its component id.
    JetForceShutoff(i32),
    /// LaserBehaviour.UpdateActivation by its component id.
    LaserUpdateActivation(i32),
    /// Behaviour.set_enabled(false) on a TankBarrelBehaviour.
    DisableTankBarrel(i32),
    /// GameObject.SetActive(bool) of the object whose Transform id this is (a subtree without physics).
    SetActive(i32, bool),
    /// Behaviour.set_enabled(false) on a Joint2D, by its component id.
    DisableJoint(i32),
    WakeUp(i32),
    AddTorque(i32, f32),
    /// LiquidSpreadAoEBehaviour.Spread by its component id.
    LiquidSpread(i32),
    /// FlamethrowerBehaviour.AffectAoe by its component id.
    FlamethrowerAoe(i32),
    /// MapLiftBehaviour.SetFloor(index) by the lift's component id.
    MapLiftSetFloor(i32, i32),
    /// MapLiftBehaviour.EmergencyStop by the lift's component id.
    MapLiftEmergencyStop(i32),
    /// A public method of a ReactorCoreBehaviour by its component id.
    ReactorCore(i32, super::reactor::ReactorCall),
    /// ReactorMeltdownBehaviour.Explode by its component id.
    MeltdownExplode(i32),
    /// StickyGrenadeBehaviour.StartBlinking by its component id.
    StickyBlink(i32),
    /// ReactorA5ArcBehaviour.Flash by its component id.
    ArcFlash(i32),
    /// ExplosionSoundBehviour.Play(delay): (id, its prefab m_Enabled, Near's length when it has an audioSource, DistantSound, delay).
    ExplosionSoundPlay(i32, bool, Option<usize>, i64, f32),
    /// ExplosionSoundBehviour set_enabled(true): (id, its prefab m_Enabled, Start's Play(Delay) when StartWhenAwake: Near's length
    /// when it has an audioSource, DistantSound, Delay).
    ExplosionSoundEnable(i32, bool, Option<(Option<usize>, i64, f32)>),
    /// PhysicalRopeCreator.CreateRope by its component id.
    RopeCreate(i32),
    /// ReactorA5ArcBehaviour.GenerateSmallArc by its component id.
    ArcSmallArc(i32),
    /// DamagableMachineryBehaviour.ForceBreak by its component id.
    MachineryForceBreak(i32),
    /// BreakJointOnShotBehaviour.Slice by its component id.
    BreakJointSlice(i32),
    /// IndustrialLiftBehaviour.SetSpeed(v) by its component id.
    IndustrialLiftSetSpeed(i32, i32),
    /// RandomSoundBehaviour.PlayClip by its component id.
    RandomSoundPlay(i32),
    /// PlayRandomClipBehaviour.Play by its component id.
    PlayRandomClipPlay(i32),
    /// Behaviour.set_enabled(v) of a RandomEventBehaviour or TemperatureTransferBehaviour (its class, component id).
    SetEnabled(&'static str, i32, bool),
    /// TargetLineRenderer.enabled = v by its component id.
    TargetLineRendererEnable(i32, bool),
    /// TrivialAnimationBehaviour.Play by its component id.
    AnimationPlay(i32),
    /// Renderer.enabled = v of a SpriteRenderer: its component id, the PhysicalBehaviour's component id when its
    /// object has one (the sprite is the physical's), and v.
    SpriteEnabled(i32, Option<i32>, bool),
    /// A call whose target is not rewritten: it stops when it runs.
    Stop(&'static str),
}

/// UseEventTrigger: Use runs Event while enabled and (not OnlyOnce or never used). Nothing sets its Action.
#[derive(Default, Clone, Debug)]
pub struct UseEvent {
    pub enabled: bool,
    pub only_once: bool,
    pub use_count: i32,
    pub calls: Vec<EventCall>,
}

/// A UnityEvent's persistent calls in Invoke order (`unity_event::calls`); `what` names the event in a Stop.
fn event_calls(prefab: &Prefab, file: usize, event: &Value, inst: &Instance, what: &str, _animations: bool) -> Result<Vec<EventCall>, String> {
    super::unity_event::calls(prefab, file, inst, event, what)
}

/// `event_calls` without the panic: the method name of the first call it cannot keep is the inner Err.
pub(super) fn try_event_calls(prefab: &Prefab, file: usize, event: &Value, inst: &Instance, animations: bool) -> Result<Result<Vec<EventCall>, String>, String> {
    let mut cars = car_calls(prefab, file, event, inst).into_iter();
    let mut out = Vec::new();
    for call in invoked_calls(event) {
        if let Some(a) = audio_call(prefab, file, inst, call) {
            out.push(a);
            continue;
        }
        if presentation_call(prefab, file, call) {
            continue;
        }
        if animations && animation_call(prefab, file, inst, call) {
            let (o, c) = call_target(prefab, file, call).unwrap();
            out.push(EventCall::AnimationPlay(inst.objects[o].components[c]));
            continue;
        }
        let method = string(call, "m_MethodName")?;
        let on = call.get("m_Arguments").and_then(|a| a.get("m_BoolArgument")).and_then(Value::bool);
        let number = call.get("m_Arguments").and_then(|a| a.get("m_FloatArgument")).and_then(Value::f32);
        let int = call.get("m_Arguments").and_then(|a| a.get("m_IntArgument")).and_then(Value::i64);
        // GameObject.SetActive: the scripts below the object are enabled or disabled with it (a subtree without physics).
        if method == "SetActive" && on.is_some() {
            let t = call.get("m_Target").and_then(Value::pptr).unwrap();
            let go = (t.file == 0).then(|| prefab.index.get(&ObjRef { file, path_id: t.path_id }).copied()).flatten().filter(|&(_, c)| c == usize::MAX);
            if let Some((to, _)) = go {
                let physics = |x: usize| prefab.objects[x].components.iter().any(|(_, k)| matches!(k, Component::Rigidbody2D(_) | Component::Collider2D(_) | Component::Joint2D(_)));
                if physics(to) || inst.descendants(to).into_iter().any(physics) {
                    return Ok(Err("SetActive of a subtree with physics".into()));
                }
                out.push(EventCall::SetActive(inst.objects[to].components[0], on.unwrap()));
                continue;
            }
        }
        let target = call_target(prefab, file, call);
        let kept = target.and_then(|(o, c)| match &prefab.objects[o].components[c].1 {
            Component::Script { class, fields, .. } => match (class.as_str(), method.as_str()) {
                ("CarBehaviour", _) => cars.next().map(|(id, c)| EventCall::Car(id, c)),
                ("DetachFromParent", "Detach") => Some(EventCall::Detach(inst.objects[o].components[c])),
                ("ActOnDestroy", "Clear") => Some(EventCall::ClearDestroyWith(inst.objects[o].components[0])),
                ("AddForceBehaviour", "AddRelativeForce") if number.is_some() => Some(EventCall::AddRelativeForce(inst.objects[o].components[c], number.unwrap())),
                ("InstantiateObjectBehaviour", "Instantiate") => Some(EventCall::Instantiate(inst.objects[o].components[c])),
                ("MapLiftBehaviour", "SetFloor") if int.is_some() => Some(EventCall::MapLiftSetFloor(inst.objects[o].components[c], int.unwrap() as i32)),
                ("ReactorCoreBehaviour", m) => super::reactor::ReactorCall::parse(m, number).map(|r| EventCall::ReactorCore(inst.objects[o].components[c], r)),
                ("DamagableMachineryBehaviour", "ForceBreak") => Some(EventCall::MachineryForceBreak(inst.objects[o].components[c])),
                ("BreakJointOnShotBehaviour", "Slice") => Some(EventCall::BreakJointSlice(inst.objects[o].components[c])),
                ("IndustrialLiftBehaviour", "SetSpeed") if int.is_some() => Some(EventCall::IndustrialLiftSetSpeed(inst.objects[o].components[c], int.unwrap() as i32)),
                ("RandomSoundBehaviour", "PlayClip") => Some(EventCall::RandomSoundPlay(inst.objects[o].components[c])),
                ("PlayRandomClipBehaviour", "Play") => Some(EventCall::PlayRandomClipPlay(inst.objects[o].components[c])),
                ("BlackHoleBehaviour", "set_enabled") if on.is_some() => Some(EventCall::SetEnabled("BlackHoleBehaviour", inst.objects[o].components[c], on.unwrap())),
                ("RandomEventBehaviour", "set_enabled") if on.is_some() => Some(EventCall::SetEnabled("RandomEventBehaviour", inst.objects[o].components[c], on.unwrap())),
                ("TemperatureTransferBehaviour", "set_enabled") if on.is_some() => Some(EventCall::SetEnabled("TemperatureTransferBehaviour", inst.objects[o].components[c], on.unwrap())),
                ("TargetLineRenderer", "set_enabled") if on.is_some() => Some(EventCall::TargetLineRendererEnable(inst.objects[o].components[c], on.unwrap())),
                // Play(float delay): `if (!base.enabled || !audioSource) return;`, base.enabled checked when the call runs
                // (explosion_sound_enabled). set_enabled(false): Behaviour.enabled (no OnDisable); set_enabled(true) also runs a
                // Start that has not run (StartWhenAwake: Play(Delay)).
                ("ExplosionSoundBehviour", "Play" | "set_enabled") if number.is_some() || on.is_some() => {
                    let Component::Script { enabled, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let has = fields.get("audioSource").and_then(Value::pptr).is_some_and(|p| !p.is_null()) || prefab.objects[o].components.iter().any(|(_, x)| matches!(x, Component::AudioSource { .. }));
                    let near = fields.get("Near").and_then(Value::array).map_or(0, |a| a.len());
                    let distant = fields.get("DistantSound").and_then(Value::i64).unwrap_or(0);
                    let id = inst.objects[o].components[c];
                    match (method.as_str(), on) {
                        ("Play", _) => Some(EventCall::ExplosionSoundPlay(id, *enabled, has.then_some(near), distant, number.unwrap())),
                        (_, Some(false)) => Some(EventCall::SetEnabled("ExplosionSoundBehviour", id, false)),
                        _ => {
                            let start = fields.get("StartWhenAwake").and_then(Value::bool).expect("ExplosionSoundBehviour.StartWhenAwake");
                            let delay = fields.get("Delay").and_then(Value::f32).expect("ExplosionSoundBehviour.Delay");
                            Some(EventCall::ExplosionSoundEnable(id, *enabled, start.then_some((has.then_some(near), distant, delay))))
                        }
                    }
                }
                ("ReactorA5ArcBehaviour", "Flash") => Some(EventCall::ArcFlash(inst.objects[o].components[c])),
                ("ReactorA5ArcBehaviour", "GenerateSmallArc") => Some(EventCall::ArcSmallArc(inst.objects[o].components[c])),
                ("PhysicalRopeCreator", "CreateRope") => Some(EventCall::RopeCreate(inst.objects[o].components[c])),
                ("ReactorMeltdownBehaviour", "Explode") => Some(EventCall::MeltdownExplode(inst.objects[o].components[c])),
                ("StickyGrenadeBehaviour", "StartBlinking") => Some(EventCall::StickyBlink(inst.objects[o].components[c])),
                ("MapLiftBehaviour", "EmergencyStop") => Some(EventCall::MapLiftEmergencyStop(inst.objects[o].components[c])),
                ("DamagableMachineryBehaviour", "BreakPermanently") => Some(EventCall::MachineryBreakPermanently(inst.objects[o].components[c])),
                ("PhysicalBehaviour", "HaltNextPropagation") => Some(EventCall::HaltNextPropagation(inst.objects[o].components[c])),
                ("DamagableMachineryBehaviour", "set_enabled") if on == Some(false) => Some(EventCall::DisableMachinery(inst.objects[o].components[c])),
                ("JetEngineBehaviour", "ForceShutoff") => Some(EventCall::JetForceShutoff(inst.objects[o].components[c])),
                ("LaserBehaviour", "UpdateActivation") => Some(EventCall::LaserUpdateActivation(inst.objects[o].components[c])),
                ("TankBarrelBehaviour", "set_enabled") if on == Some(false) => Some(EventCall::DisableTankBarrel(inst.objects[o].components[c])),
                ("LiquidSpreadAoEBehaviour", "Spread") => Some(EventCall::LiquidSpread(inst.objects[o].components[c])),
                ("FlamethrowerBehaviour", "AffectAoe") => Some(EventCall::FlamethrowerAoe(inst.objects[o].components[c])),
                ("UseEventTrigger", "set_enabled") if on.is_some() => Some(EventCall::SetUseEventEnabled(inst.objects[o].components[c], on.unwrap())),
                ("DeleteAfterTime", "set_enabled") if on == Some(true) => Some(EventCall::EnableDeleteAfter(inst.objects[o].components[c])),
                ("RandomForceAtStart", "set_enabled") if on == Some(true) => {
                    let k = prefab.objects[o].components.iter().position(|(_, x)| matches!(x, Component::Rigidbody2D(_)))?;
                    Some(EventCall::EnableRandomForce(inst.objects[o].components[k], float(fields, "PositionalVelocityStrength").ok()?, float(fields, "RotationalVelocityStrength").ok()?))
                }
                _ => None,
            },
            Component::Rigidbody2D(_) if method == "set_simulated" && on == Some(true) => Some(EventCall::Simulate(inst.objects[o].components[c])),
            Component::Rigidbody2D(_) if method == "WakeUp" => Some(EventCall::WakeUp(inst.objects[o].components[c])),
            Component::Rigidbody2D(_) if method == "AddTorque" && number.is_some() => Some(EventCall::AddTorque(inst.objects[o].components[c], number.unwrap())),
            Component::Joint2D(_) if method == "set_breakForce" && number.is_some() => Some(EventCall::SetBreakForce(inst.objects[o].components[c], number.unwrap())),
            Component::Joint2D(_) if method == "set_enabled" && on == Some(false) => Some(EventCall::DisableJoint(inst.objects[o].components[c])),
            Component::SpriteRenderer(_) if method == "set_enabled" && on.is_some() => {
                let physical = prefab.objects[o].components.iter().position(|(_, k)| matches!(k, Component::Script { class, .. } if class == "PhysicalBehaviour"));
                Some(EventCall::SpriteEnabled(inst.objects[o].components[c], physical.map(|k| inst.objects[o].components[k]), on.unwrap()))
            }
            _ => None,
        });
        match kept {
            Some(e) => out.push(e),
            None => {
                let on = target.map(|(o, c)| match &prefab.objects[o].components[c].1 {
                    Component::Script { class, .. } => class.clone(),
                    other => format!("{other:?}").chars().take(20).collect(),
                });
                return Ok(Err(format!("{method} on {}", on.unwrap_or_default())));
            }
        }
    }
    Ok(Ok(out))
}

/// A UnityEvent's persistent calls that run on Invoke: all but the Off ones and the ones with no method (PersistentCall.IsValid).
pub(super) fn invoked_calls(event: &Value) -> Vec<&Value> {
    let calls = event.get("m_PersistentCalls").and_then(|c| c.get("m_Calls")).and_then(Value::array).unwrap();
    calls.iter().filter(|c| c.get("m_CallState").and_then(Value::i64) != Some(0) && string(c, "m_MethodName").is_ok_and(|m| !m.is_empty())).collect()
}

/// The prefab object and component a persistent call targets.
fn call_target(prefab: &Prefab, file: usize, call: &Value) -> Option<(usize, usize)> {
    let target = call.get("m_Target").and_then(Value::pptr).unwrap();
    (target.file == 0).then(|| prefab.index.get(&ObjRef { file, path_id: target.path_id }).copied()).flatten().filter(|&(_, c)| c != usize::MAX)
}

/// Play on a Translation/Rotation/ColorAnimationBehaviour animating a subtree without physics.
fn animation_call(prefab: &Prefab, file: usize, inst: &Instance, call: &Value) -> bool {
    call_target(prefab, file, call).is_some_and(|(to, tc)| {
        matches!(&prefab.objects[to].components[tc].1, Component::Script { class, .. } if matches!(class.as_str(), "TranslationAnimationBehaviour" | "RotationAnimationBehaviour" | "ColorAnimationBehaviour"))
            && string(call, "m_MethodName").is_ok_and(|m| m == "Play")
            && std::iter::once(to).chain(inst.descendants(to)).all(|x| prefab.objects[x].components.iter().all(|(_, c)| !matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_))))
    })
}

fn presentation_call(prefab: &Prefab, file: usize, call: &Value) -> bool {
    call_target(prefab, file, call).is_some_and(|(o, c)| match &prefab.objects[o].components[c].1 {
        Component::AudioSource { .. } | Component::Other(120 | 164..=170 | 198) => true,
        // Its LateUpdate only drives an AudioSource (see check_rewritten).
        Component::Script { class, .. } => class == "RectangleAudioSourceBehaviour"
            // PhysicalBehaviour.PlayClipOnce: the AudioSource's PlayOneShot (audio only).
            || (class == "PhysicalBehaviour" && string(call, "m_MethodName").is_ok_and(|m| m == "PlayClipOnce"))
            // AudioSource.Play or Stop.
            || (class == "AudioSourceToggleBehaviour" && string(call, "m_MethodName").is_ok_and(|m| m == "Toggle"))
            // Refresh: LightSprite.enabled = GlowingSprites.Contains(ObjectSprite.sprite).
            || (class == "JackOLaternBehaviour" && string(call, "m_MethodName").is_ok_and(|m| m == "Refresh")),
        _ => false,
    })
}

/// The objects of `p` from `root` down (they follow `root` in order), as a prefab of their own.
pub(super) fn subtree_prefab(p: &Prefab, root: usize) -> Prefab {
    let mut end = root + 1;
    while end < p.objects.len() && p.objects[end].parent.is_some_and(|q| q >= root) {
        end += 1;
    }
    let mut out = Prefab::default();
    for (k, o) in p.objects[root..end].iter().enumerate() {
        let mut o = o.clone();
        o.parent = if k == 0 { None } else { o.parent.map(|q| q - root) };
        o.children = o.children.iter().map(|c| c - root).collect();
        out.objects.push(o);
    }
    out.index = p.index.iter().filter(|(_, v)| (root..end).contains(&v.0)).map(|(r, v)| (*r, (v.0 - root, v.1))).collect();
    out.materials = p.materials.iter().filter(|(k, _)| (root..end).contains(&k.0)).map(|(k, m)| ((k.0 - root, k.1), *m)).collect();
    out
}

/// `check_rewritten` for the script on prefab object `o`: a TrivialAnimationBehaviour only animates its own
/// Transform or SpriteRenderer (AnimationCurve/Gradient, no Random), which is presentation over a subtree without
/// colliders or bodies.
pub(super) fn check_rewritten_at(prefab: &Prefab, o: usize, class: &str) {
    // Update reads IsBeingUsedContinuously into private counters and calls ForceSetProgress on its TrivialAnimationBehaviour:
    // presentation when that animation is.
    if class == "TrivialAnimationActivationBehaviour" {
        let Some((_, Component::Script { fields, .. })) = prefab.objects[o].components.iter().find(|(_, c)| matches!(c, Component::Script { class: k, .. } if k == class)) else { unreachable!() };
        let target = fields.get("AnimationBehaviour").and_then(Value::pptr).filter(|p| !p.is_null()).unwrap_or_else(|| unimplemented!("mod-only: TrivialAnimationActivationBehaviour with a null AnimationBehaviour (NullReferenceException)"));
        let (to, tc) = prefab.index.iter().find(|(k, _)| k.path_id == target.path_id).map(|(_, v)| *v).expect("TrivialAnimationActivationBehaviour.AnimationBehaviour outside the prefab");
        let Component::Script { class: animation, .. } = &prefab.objects[to].components[tc].1 else { unimplemented!("mod-only: TrivialAnimationActivationBehaviour.AnimationBehaviour is no script") };
        check_rewritten_at(prefab, to, animation);
        return;
    }
    // TrivialAnimationActivationBehaviour.Update: visual.rs visual_update (the held-use counters, then ForceSetProgress)
    // TrivialAnimationActivationBehaviour.Awake: phys is the physical on the same object.
    // TrivialAnimationBehaviour.Routine: visual.rs trivial_animation_play (the coroutine steps AnimationTick over Duration)
    // TrivialAnimationBehaviour.constructor: Duration = 1, the field initialiser (link reads the prefab value)
    // TranslationAnimationBehaviour.Awake: startPos = localPosition (link_visuals)
    // RotationAnimationBehaviour.Awake: link_visuals (an empty base Awake)
    // ColorAnimationBehaviour.Awake: spriteRenderer = GetComponent (link_visuals)
    // ColorWave.OnWillRenderObject: visual.rs visual_will_render
    // ParallaxBehaviour.Awake: presentation (its own Transform, colours, shader properties or UI for the camera)
    // ParallaxBehaviour.OnPreRender: presentation (its own Transform, colours, shader properties or UI for the camera)
    // CameraSyncBehaviour.OnPreRender: presentation (its own Transform, colours, shader properties or UI for the camera)
    // LogoFlashBehaviour.Start: presentation (its own Transform, colours, shader properties or UI for the camera)
    // LogoFlashBehaviour.Update: presentation (its own Transform, colours, shader properties or UI for the camera)
    // IgnoreCameraZoom.Start: no prefab has one; the Main scene's sets CameraToConsider (the Main Camera), so Start keeps it (thermal_display.rs)
    // IgnoreCameraZoom.OnWillRenderObject: the Main scene's display, Game::thermal_display_will_render
    // UiColorWave.OnEnable: presentation (its own Transform, colours, shader properties or UI for the camera)
    // UiColorWave.Update: presentation (its own Transform, colours, shader properties or UI for the camera)
    // UiColorWave.OnDisable: presentation (its own Transform, colours, shader properties or UI for the camera)
    // EmitParticlesOnShakeBehaviour.Awake: the link pass below (its ParticleSystem and Rigidbody2D)
    // AudioSourceTimeScaleBehaviour.OnDestroy: the destroyed component's sources leave `Audio::time_scaled` (audio_update)
    // RectangleAudioSourceBehaviour.GetNormalisedDistanceFactor: audio
    // RectangleAudioSourceBehaviour.LateUpdate: audio
    // MuteBehaviour.SetMute: audio
    // MinigunBehaviour.SoundRoutine: audio
    // PoliceBoxBehaviour.FadeRoutine: presentation (SpriteRenderer.color fade and SetActive(false) of a render-only subtree)
    // PoliceBoxBehaviour.OnBecameVisible: presentation (starts FadeRoutine on a render-only subtree)
    // SortingLayerChild.Awake: the visuals arm (link.rs) / OnWillRenderObject: visual.rs sorting_layer_child_will_render
    // VertexColourBasedOnTemperatureBehaviour.Update: visual.rs visual_update
    // LoopSoundBehaviour.StartLoop: audio.rs loop_sound_start / loop_sound_stop
    // LoopSoundBehaviour.StopLoop: audio.rs loop_sound_start / loop_sound_stop
    // LoopSoundBehaviour.get_IsPlaying: audio.rs LoopSound.IsPlaying
    // LoopSoundBehaviour.set_IsPlaying: audio.rs LoopSound.IsPlaying
    // CameraListenerBehaviour.Awake: audio (the reverb filter; `main` is only used for Mute and Unmute)
    // CameraListenerBehaviour.Mute: audio (AudioListener.volume)
    // CameraListenerBehaviour.Unmute: audio (AudioListener.volume)
    // CameraShakeBehaviour.Awake: main = this (the one camera root: `Game::camera`)
    // ProceduralShatterMaterialPool.Awake: Instance = this (statics in mod.rs)
    // RandomSoundBehaviour.Awake: phys = GetComponent<PhysicalBehaviour>() (the `random_sounds` link)
    // ParticleMachineBehaviour.Awake: phys = GetComponent<PhysicalBehaviour>() (the `Phys` link of Menu::ParticleMachine)
    // GlowtubeBehaviour.get_Colour: light.rs glowtube_colour
    // GlowtubeBehaviour.Slice: Break (`send_slice`)
    // ExportGameDebugInfoBehaviour.GetDebugInfoString: presentation (a debug report of the system and mods; no simulation state)
    // ExportGameDebugInfoBehaviour.Export: presentation (writes that report and shows a dialog)
    // LoadingBarBehaviour.SetProgress: presentation (a UI bar's scale and text)
    // SettingTemplateBehaviour.SetHeight: presentation (settings UI layout)
    // URLOpenBehaviour.OpenURL: presentation (opens a browser; no simulation state)
    // URLOpenBehaviour.OpenLocal: presentation (Process.Start of a file or folder; no simulation state)
    // EditableTintBehaviour.Awake: the link's physical (GetComponent<PhysicalBehaviour>(), at its PendingStart arm)
    // Highlight.Initialise: Awake runs it on each Highlights entry; a new MaterialPropertyBlock read from its SpriteRenderer, which only the renderer uses.
    // MuteBehaviour.Start: rewritten as the link-time arm below / at the arm of that class
    // SoundOnShakeBehaviour.Awake: rewritten as the link-time arm below / at the arm of that class
    // HologramJitter.Awake: rewritten as the link-time arm below / at the arm of that class
    // ColorWave.OnEnable: rewritten as the link-time arm below / at the arm of that class
    // NukeShockwaveBehaviour (Start, PushAway, Update, FixedUpdate): tools/progress/callgraph.py lists them dead (only
    // NukeVFX.prefab carries the script); the particles still run them on any clone that has it (particle_shockwave.rs).
    // NukeShockwaveBehaviour.SpawnParticles: particle_shockwave.rs Shockwave::fixed_update (the rays from the bounds' edge;
    // each hit is an Emit(EmitParams) at the point, particles.rs begin_update_all)
    // The classes above: see check_rewritten_at and the arms below.
    // MapItemBehaviour.Start: presentation (a UI image and tooltip text)
    // FitCameraBackgroundBehaviour.Awake: presentation (its own Transform follows the camera; the arm below stops if colliders or bodies hang on it)
    // FitCameraBackgroundBehaviour.Update: presentation (its own Transform follows the camera; the arm below stops if colliders or bodies hang on it)
    // FitCameraBackgroundBehaviour.OnWillRenderObject: presentation (its own Transform follows the camera; the arm below stops if colliders or bodies hang on it)
    // RectangularReverbZone.OnEnable: audio (AudioReverbZone distances)
    // RectangularReverbZone.Update: audio (AudioReverbZone distances)
    // SceneSwitchBehaviour.Awake: presentation (the loading bar)
    // SceneSwitchBehaviour.UpdateProgress: presentation (the loading bar)
    // EnvironmentSettingsController.Awake: Main = this (the one window, ui `env_open`)
    // EnvironmentSettingsController.Start: the window starts closed (ui `env_open` false) and reads MapConfig (map.rs environment rows)
    // EnvironmentSettingsController.Update: ui/mod.rs, the toybox key closing the window
    // EnvironmentSettingsController.RegenerateControls: ui/view.rs environment, one row per EnvironmentalSettings field
    // EnvironmentSettingsController.PrepareControl: ui/view.rs environment, the row's label and its tooltip in the window's Tooltip box
    // NukeShockwaveBehaviour only moves and scales its own Transform and emits particles: the same guard.
    // DangleBehaviour (Awake, OnWillRenderObject) only swings its own Transform for the camera (dangle.rs), unless colliders or bodies hang on it.
    if matches!(class, "TranslationAnimationBehaviour" | "RotationAnimationBehaviour" | "ColorAnimationBehaviour" | "DangleBehaviour" | "FitCameraBackgroundBehaviour" | "NukeShockwaveBehaviour") {
        fn physics(prefab: &Prefab, o: usize) -> bool {
            prefab.objects[o].components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_))) || prefab.objects[o].children.iter().any(|&x| physics(prefab, x))
        }
        if physics(prefab, o) {
            unimplemented!("mod-only: {class} moving colliders or bodies");
        }
        return;
    }
    // PlayRandomClipBehaviour: Start and Play are rewritten (reactor.rs); the audio is presentation.
    if class == "PlayRandomClipBehaviour" || cfg!(test) && class == "ThrowAtAwake" {
        return;
    }
    check_rewritten(class);
}

/// GetComponentsInChildren<Collider2D>() from object `o` of a clone: the active objects depth first, each one's
/// colliders in component order, as (collider id, object).
pub(super) fn colliders_in_children(inst: &Instance, o: usize) -> Vec<(i32, usize)> {
    let prefab = inst.prefab.clone();
    std::iter::once(o)
        .chain(inst.descendants(o))
        .filter(|&x| inst.objects[x].active)
        .flat_map(|x| prefab.objects[x].components.iter().enumerate().filter(|(_, (_, c))| matches!(c, Component::Collider2D(_))).map(move |(k, _)| (x, k)).collect::<Vec<_>>())
        .map(|(x, k)| (inst.objects[x].components[k], x))
        .collect()
}

/// Stops at a MonoBehaviour whose Unity messages (lifecycle.rs) the rewrite does not run: every class with any is
/// either rewritten or only touches audio, rendering or statistics the rewrite does not simulate.
pub(super) fn check_rewritten(class: &str) {
    match class {
        "PhysicalBehaviour" | "LimbBehaviour" | "CirculationBehaviour" | "PersonBehaviour" | "SkinMaterialHandler" | "ConnectedNodeBehaviour" | "ReturnToPoolAfterTime"
        | "GoreStringBehaviour" | "GripBehaviour" | "HingeJointLimitAutofixBehaviour" | "MapLightBehaviour" | "MapConfig" | "BoundingBoxBehaviour"
        | "ActivateIfTutorial" | "WaterBehaviour" | "PlayAudioAtRandom" | "PlayAtRandom" | "ShatteredObjectGenerator"
        | "JointBreakSoundBehaviour" | "DamagableMachineryBehaviour" | "DestroyableBehaviour" | "VaseBehaviour" | "ExplosionSoundBehviour" | "ActOnDestroy"
        | "DestroyWhenAllChildrenDestroyed" | "InstantiateObjectBehaviour" | "DetachFromParentOnStart" | "RandomForceAtStart" | "DoAtTimeOnce" | "RandomSpriteBehaviour" | "LavaBehaviour" | "RandomPrefabSpawnerBehaviour"
        | "InverterBehaviour" | "ObstructionEmitterBehaviour" | "TemperatureTargetRectangleBehaviour" | "ExistByChance" | "ExplosiveBehaviour" | "DeleteAfterTime" | "InfiniteLiquidSourceBehaviour" | "SlightMovement" | "CustomFixedConstrainedBehaviour" | "RadiationGridController" | "DisableWhenControlTrigger" | "NeedleBehaviour" | "StaticNeedleBehaviour" | "RaygunBehaviour" | "MachineGunProjectileBehaviour" | "PlaneBehaviour" | "WingBehaviour" | "GlassShardBehaviour" | "ShockwaveBehaviour" | "ActOnCollide" | "NoCollide" | "StickyGrenadeBehaviour" | "ActOnHingeJointLimit" | "PointToVelocityBehaviour" | "AndroidSparkCreator" | "ActivateIfFancyEffects" | "ExistInDetailView" | "DebugTargetDamageMeter" | "ExistInBeta" | "ExistInThermalVision" | "IgnoreParentSize" | "LocalFirePropagationMap" | "RandomSpawnedSize" | "NoChildrenCollide" | "FirearmBehaviour" | "CarBehaviour" | "TyreDeflationBehaviour" | "GasTankBehaviour" | "FlaskBehaviour" | "LiquidContainerController" | "PointLiquidTransferBehaviour" | "TruckDoorBehaviour" | "GlowtubeBehaviour" | "LEDBulbBehaviour" | "DetectorBehaviour" | "RandomCarTextureBehaviour" | "NoCollideGm" | "JointSerialisationHelper" | "BatchJointSerialisationHelper" | "ProjectileLauncherBehaviour" | "OnlyExplosionBehaviour" | "BellBehaviour" | "MachineGunBehaviour" | "GeneratorBehaviour" | "SingleFloodlightBehaviour" | "SpikeGrenadeBehaviour" | "HeatingElementBehaviour" | "TemperatureRayGunBehaviour" | "BulbBehaviour" | "ThrusterbedBehaviour" | "PistonBehaviour" | "FlamethrowerBehaviour" | "LaunchedRocketBehaviour" | "PressureGridController" | "WhOpening" | "wh" | "CannotShare"
        // Airfoil's Awake, Start and FixedUpdate are rewritten (airfoil.rs; its context menu buttons are UI). Stunner's Update is (stunner.rs).
        // determined_anomaly.rs; the start sound's time draw and the array's clones are in misc.rs.
        | "DeterminedAnomaly" | "StartAudioSourceAtRandomTimeBehaviour" | "ArrayCloner"
        | "CathodeLightBehaviour" | "UnderwaterExplosionBehaviour" | "AirfoilBehaviour" | "StunnerBehaviour" | "SoapBehaviour" | "PowerHammerBehaviour" | "BalloonBehaviour" | "MetronomeBehaviour" => {}
        // Start, OnEnable, FixedUpdate, Use and OnDisable are rewritten (thermometer.rs), the readout text included.
        // InfraredThermometer.Update is rewritten (thermometer.rs: infrared_update), its text included; the line is rendering.
        "ThermometerBehaviour" | "InfraredThermometer" => {}
        // Start flips a negative local scale.x back (thermometer.rs: maintain_scale_start).
        "MaintainScaleSignBehaviour" => {}
        // Awake, Update (the arm's spring) and Start (the labels) are rewritten (gauge.rs).
        "PowerGaugeBehaviour" | "PressureGaugeBehaviour" | "TemperatureGaugeBehaviour" => {}
        // Start enables its TorchBehaviour while Activated; Use lights it (flare.rs).
        "FlareBehaviour" => {}
        // OnEnable registers contacts, OnCollisionEnter2D and Exit2D and FixedUpdate are rewritten (torch.rs).
        "TorchBehaviour" => {}
        // Awake draws the glow seed, Start is presentation and the context menu (television.rs), and Update, Use, OnCollisionEnter2D and
        // Shot/Break are rewritten (television.rs).
        "TelevisionBehaviour" => {}
        // Awake subscribes to the continuous listener and FixedUpdate presses (button.rs); Start is the context menu, and
        // Update and OnDisable only drive the light.
        "ButtonBehaviour" => {}
        // Start and Use run UpdateLaser and LateUpdate fires it (laser.rs); OnDisable and OnDestroy only hide the beam.
        "LaserBehaviour" => {}
        // Awake adds a CheckOnDestroyAttachment; Use and Battery's FixedUpdate need a connection, which the point makes and
        // this file refuses (attachment.rs). The base class is the scope's and the others' Awake.
        "FirearmAttachmentBehaviour" | "ScopeAttachmentBehaviour" | "BatteryAttachmentBehaviour" | "ExplosiveAttachmentBehaviour" | "FlashlightAttachmentBehaviour" | "IncendiaryAttachmentBehaviour" => {}
        // Awake draws the start string and Use and OnDisable flip Activated (display.rs); Start writes the text and its button (menu.rs).
        "DisplayBehaviour" => {}
        // Awake draws its offset (the draw stays, in awake_behaviours); OnWillRenderObject's colour and the text object's
        // localPosition from the offset's Perlin noise are rendering.
        "HologramJitter" => {}
        // Awake subscribes to the physical's OnSingleUse (activate_direct), Start and OnDisable are rewritten
        // (activation_toggle.rs), OnDestroy only removes the listener (destroyable.rs).
        "ActivationToggleBehaviour" => {}
        // jet_engine.rs: Awake caches components and clears the plasma colour (rendering).
        "JetEngineBehaviour" => {}
        // chainsaw.rs: Start draws the seed, FixedUpdate saws, Update adds speed, OnDestroy destroys the joints; the
        // audio, particles and the saw sprite are presentation.
        "ChainsawBehaviour" => {}
        // freeze.rs: Start keeps the velocities and requests the freeze, OnDestroy releases it; the outline is rendering.
        "FreezeBehaviour" => {}
        // freeze.rs: Start keeps the FreezeBehaviour when IsFrozen, else destroys it (queued).
        "FreezeSerialisationHelper" => {}
        // ceiling_turret.rs: Start, Update and FixedUpdate aim and fire (automatic_sentry.rs); OnEnable is at awake
        // (awake_behaviours) and at SetActive (water.rs), OnDisable at SetActive (awake_set_inactive, water.rs, machinery.rs);
        // OnSight and OnLoseTarget are presentation (checked in the link block).
        "CeilingTurretBehaviour" => {}
        // counter.rs: Start, Use and SendSignal (the buttons are in menu.rs); the nixie tube and clip are presentation.
        "CounterBehaviour" => {}
        // counter.rs: OnEnable and OnDisable are at the CeilingTurret sites; Start's button is in menu.rs; the light and audio are presentation.
        "PointerBehaviour" => {}
        // temperature_transfer.rs: Awake caches the PhysicalBehaviour (link block); FixedUpdate moves the temperatures.
        "TemperatureTransferBehaviour" => {}
        // worm_staff.rs: Start and Update (the line vertices are rendering).
        "WormStaffBehaviour" => {}
        // atom_explosion.rs: Start heats and vaporises, Update grows the shockwave (Awake only finds the camera).
        "NewAtomBombExplosionBehaviour" => {}
        // prefab_bomb.rs: Use instantiates ExplosionPrefab and destroys the GameObject.
        "AtomBombBehaviour" | "SeismicChargeBehaviour" => {}
        // laser_turret.rs: FixedUpdate aims and fires (automatic_sentry.rs), Use shoots, OnEMPHit breaks it.
        "LaserTurretBehaviour" => {}
        // sentry_turret.rs: the root's state machine and the gun's aim and fire; Explode is the EMP, burning and shot paths.
        "SentryTurretBehaviour" => {}
        "SentryTurretGunBehaviour" => {}
        // random_event.rs: Update draws once per frame while on; Events dispatch only on an exact 0 draw (not ported).
        "RandomEventBehaviour" => {}
        // emp.rs: Use runs Activate; Awake instantiates the Killzone under the object.
        "EMPBehaviour" => {}
        // decimator.rs
        "DecimatorBehaviour" => {}
        // drone.rs: Start hangs the propellers on hinges, Use toggles, FixedUpdate hovers and steers, Shot hurts; Awake
        // caches the physical (and loops the hum, audio). PropellerBehaviour.Awake caches its rigidbody.
        "DroneBehaviour" => {}
        "PropellerBehaviour" => {}
        // portal.rs: Start sets the two trigger delegates, StartPassage and StartExit swap the layer; the ghosts and their
        // poses are presentation. TriggerEventBehaviour forwards the trigger messages to those delegates.
        // activations.rs: NodeBehaviour's Use and serialisation hooks are empty; nothing fires its events. NewUseWireBehaviour
        // is a wire (blood_wire.rs): Start, Tick, Slice, JointBroken and OnDestroy.
        "NodeBehaviour" => {}
        "NewUseWireBehaviour" => {}
        "PortalBehaviour" => {}
        "TriggerEventBehaviour" => {}
        // black_hole.rs: Awake only reads the layer mask.
        "BlackHoleBehaviour" => {}
        // immobility_field.rs
        "ImmobilityFieldBehaviour" => {}
        // forcefield.rs
        "ForcefieldGeneratorBehaviour" => {}
        // lightning_gun.rs: Update, FixedUpdate and OnDisable are rewritten; the particles and sounds are presentation.
        "LightningGunBehaviour" => {}
        // javelin.rs: Start subscribes TransferTarget to OnLaunch (the line is presentation). OnEnable and FixedUpdate only
        // recast the laser for the line, its colour and the lock-on sound, and TransferTarget casts it again; OnDestroy only
        // unsubscribes, and a destroyed launcher launches nothing.
        "JavelinBehaviour" => {}
        // javelin.rs: Start, FixedUpdate, OnCollisionEnter2D, Explode, OnEMPHit and Shot are rewritten; Update is the fins' scale.
        "JavelinMissileBehaviour" => {}
        // tesla_coil.rs: Awake instantiates the bolts (their Update is rewritten there), Start, Use, FixedUpdate,
        // OnDisable and OnDestroy; the audio is presentation.
        "TeslaCoilBehaviour" => {}
        // energy_sword.rs: Awake hides the blade, Start, Use, FixedUpdate, Update, OnDisable and OnDestroy are
        // rewritten; the audio and particles are presentation.
        "EnergySwordBehaviour" => {}
        // OnCollisionEnter2D instantiates CollideWithOtherSwordVFX (simulate.rs).
        "EnergySwordBladeClashBehaviour" => {}
        // energy_sword.rs: OnEnable, Update and its node and mesh methods; Awake's Mesh and OnDestroy's Destroy(mesh)
        // are src/energy_blur.rs's (it moves only its own Transform, which nothing simulated reads).
        "EnergySwordMotionBlur" => {}
        // blaster.rs, scifi.rs: Awake caches components (the Disassembler's takes an AudioSource id), Start flips the
        // muzzle flash (BlasterBehaviour), shows or hides lines (BeamformerBehaviour); the bolt only has Update.
        "BlasterBehaviour" | "BlasterboltBehaviour" | "BeamformerBehaviour" | "GenericScifiWeapon40Behaviour" => {}
        // effect_gun.rs: Awake adds an AudioSource (its id), Use and Affect are rewritten.
        "HealthGunBehaviour" | "FreezeGunBehaviour" => {}
        // syringe.rs: Awake and OnWillRenderObject are rendering, Start's AddButtons is UI.
        c if super::syringe::is_syringe(c) || c == "SyringeExplosionBehaviour" => {}
        // Renderer sorting from the parent's SpriteRenderer; SpriteRenderer.color from Temperature when visible.
        // SmoothLineControllerBehaviour: smooth_line.rs.
        "SortingLayerChild" | "VertexColourBasedOnTemperatureBehaviour" | "SmoothLineControllerBehaviour" => {}
        // ObjectGradientAnimation's Start, Update and OnEnable only advance its private t and call SetColor, whose one subclass here writes
        // SpriteRenderer.color (nothing reads it). RotatingSoundBehaviour only drives its own AudioSource.
        // ObjectGradientAnimation.Update: presentation (t advances by deltaTime * Speed, then ApplyColour)
        // ObjectGradientAnimation.OnEnable: presentation (ReplayOnEnable resets t, then ApplyColour)
        // ObjectGradientAnimation.ApplyColour: presentation (Gradient.Evaluate(cycle of t + Offset + the position influence) * Tint into SetColor; no draw)
        // ObjectGradientAnimation.constructor: presentation (Tint = white, Speed = 0.1 field initialisers)
        // RotatingSoundBehaviour.Awake: audio (the looping AudioSource and its Rigidbody2D lookup; no draw)
        // RotatingSoundBehaviour.FixedUpdate: audio (AudioSource.volume from the angular velocity)
        // RotatingSoundBehaviour.constructor: audio (foleyVolume = 1)
        "SpriteGradientAnimation" | "RotatingSoundBehaviour" => {}
        // OnEnable's two Random.value (`color_wave_on_enable`) feed only OnWillRenderObject's SpriteRenderer.color.
        "ColorWave" => {}
        // Start: see PendingStart::FirearmAttachmentPoint; OnTriggerEnter2D: `attachment_point_trigger_enter`; OnDestroy only
        // destroys the FirearmAttachment component, which goes with its object.
        "FirearmAttachmentPointBehaviour" => {}
        // Awake: `weightless_helper_awake`; OnMakeWeightless/OnMakeWeightful: `physical_make_weightless`/`physical_make_weightful`.
        "WeightlessHelper" => {}
        // OnChildDisintegrated over a person's limbs (PersonBehaviour.DisintegrationCount) or `Game::disintegration_counters`.
        "DisintegrationCounterBehaviour" => {}
        // OnDestroy drops the object's undo entry.
        "DeregisterBehaviour" => {}
        // Awake and OnDestroy keep GearManagerBehaviour.Gears; OnTriggerEnter2D's GetComponentInParent is unused and
        // ManagedUpdate/ManagedLateUpdate are empty.
        "GearBehaviour" => {}
        // Global.AddAudioSource/RemoveAudioSource, AudioSource.volume, StatManager, SpriteRenderer.color; material
        // property blocks in OnWillRenderObject.
        // AudioSource.Play/Stop/volume from the camera position (RectangleAudioSourceBehaviour), the radio clip (its instance
        // id is taken in Awake), and the OnBecameVisible fade of a SpriteRenderer before SetActive(false) of a render-only
        // subtree (PoliceBoxBehaviour).
        // BurnableSpriteBehaviour.Awake: sprite_blocks.rs burnable_sprite_awake
        // BurnableSpriteBehaviour.OnWillRenderObject: sprite_blocks.rs burnable_sprites_will_render
        // WaterSoundBehaviour.Update and RectangleAudioSourceBehaviour.LateUpdate: `audio::AudioScript`.
        "WaterSoundBehaviour" | "RectangleAudioSourceBehaviour" => {}
        // PanelsBehaviour: ui_visual.rs.
        "AudioSourceTimeScaleBehaviour" | "PanelsBehaviour" | "BurnableSpriteBehaviour"
        | "GlowingHotMetalBehaviour" | "FuseBoxBehaviour" | "PoliceBoxBehaviour" => {}
        // Start and Update are rewritten (shake.rs): IsPlaying from the particles' isEmitting (particles.rs).
        "ParticleSystemSerialisationHelper" => {}
        // Start is rewritten (misc.rs); SetMute's AudioSource.enabled is audio.
        "MuteBehaviour" => {}
        // Use and Update are rewritten (minigun.rs). Awake caches components, FixedUpdate only decays the barrel's spin
        // and OnWillRenderObject sets its property block: presentation. The sound coroutine is audio.
        "MinigunBehaviour" => {}
        // Update (the charge and the shots) is rewritten (accelerator.rs).
        "AcceleratorGunBehaviour" => {}
        // Update and FixedUpdate are rewritten (accelerator.rs, bolt.rs for the flight); the bolt has no Awake or Start.
        "AcceleratorBoltBehaviour" => {}
        // Start and Use (the BurstShot coroutine) are rewritten (archelix.rs); the muzzle flash and sounds are presentation.
        "ArchelixCasterBehaviour" => {}
        // Start, Update, FixedUpdate, OnHit, DespawnRoutine and OnPoolableInitialised are rewritten (archelix.rs, pool.rs).
        "ArchelixCasterBoltBehaviour" => {}
        // Start, FixedUpdate and Update are rewritten (universal_bell.rs); Awake, colours and labels are presentation.
        "UniversalBellBehaviour" => {}
        // FixedUpdate is rewritten (mirror.rs); Awake and OnWillRenderObject set the shader's progress only.
        "ToggleableMirrorBehaviour" => {}
        // Awake (linked), Start and OnWillRenderObject are rewritten (foliage.rs); the property block is presentation.
        "FoliageBehaviour" => {}
        // Awake, Update, OnJointBreak2D, Shot, Slice and OnEnable's GetComponent are rewritten (breakjoint.rs).
        "BreakJointOnBurnBehaviour" | "BreakJointOnShotBehaviour" => {}
        // Awake (linked) and FixedUpdate are rewritten (shake.rs); the one-shot itself is audio.
        "SoundOnShakeBehaviour" => {}
        // FixedUpdate and OnCollisionEnter2D are rewritten (gorse.rs); Start caches the emission module only.
        "GorseDroolingBehaviour" | "GorseProjectileBehaviour" => {}
        // Awake and Update are rewritten (gorse.rs); the renderer toggle is rendering.
        "GorseEyeBehaviour" => {}
        // Awake (its buffer) and OnCollisionEnter2D are rewritten (excalibur.rs), the emission rates too.
        "FireballBehaviour" => {}
        // Start and FixedUpdate are rewritten (misc.rs); OnCollide has no subscriber outside the rewrite's checks.
        "FancyBloodSplatController" => {}
        // Awake and FixedUpdate are rewritten (shake.rs): Emit on the particles (particles.rs).
        "EmitParticlesOnShakeBehaviour" => {}
        // Start and Spread are rewritten (liquid_spread.rs).
        "LiquidSpreadAoEBehaviour" => {}
        // OnCollisionEnter2D is rewritten (wrench.rs); its android limb heal and Repair receivers stop there.
        "WrenchBehaviour" => {}
        // OnCollisionEnter2D is rewritten (no_collide.rs).
        "NoCollideWithBodyPart" => {}
        // Start and Update are rewritten (blood_tank.rs); its splat's OnCollide listener is set at link time.
        "BloodTankBehaviour" => {}
        // Update is BloodContainer's (liquid_outlet.rs); FixedUpdate, OnCollide and the awake subscription are rewritten there.
        "LiquidOutletBehaviour" => {}
        // Update and Use are rewritten (valve.rs); Awake, Start, the display and OnWillRenderObject are presentation.
        "ValveBehaviour" | "LiquidPressureValveBehaviour" => {}
        // Update, Start and UpdateText are rewritten (liquid_canister.rs); Awake is the link.
        "LiquidCanisterBehaviour" => {}
        // BloodContainer.Update only (liquid_filter.rs).
        "LiquidFilterBehaviour" => {}
        // Update, Use and OnDisable are rewritten (liquidentifier.rs); Start is the context button (UI).
        "LiquidentifierBehaviour" => {}
        // FixedUpdate, Update and Use are rewritten (centrifuge.rs); Start and OnWillRenderObject are the lights and the audio.
        "CentrifugeBehaviour" => {}
        // FixedUpdate is rewritten (centrifuge.rs); Awake is the link.
        "VibrationBehaviour" => {}
        // Awake is rewritten (car.rs).
        "AutomaticWheelJointCreator" => {}
        // Awake/Start/OnEnable/OnDisable only switch line renderers, glow sprites, particles and audio, and add context
        // buttons (ParticleMachine's in menu.rs). Use (and RayCannon's OnDisable) is rewritten (blaster.rs; ParticleMachine in tool.rs).
        // RailgunBehaviour: Use, FireCoroutine, HandleImpact and OnDisable are rewritten (blaster.rs); Awake and OnEnable set
        // the Lights' animations, Update the trail (presentation).
        // BeamRepeaterBehaviour: Awake (dist), Use, Shoot, FireCoroutine, OnHit and Update are rewritten (blaster.rs); the
        // muzzle flash, audio and line are presentation. AndroidLaserBehaviour: Update, FixedUpdate, Use, Fire and HandleHit
        // are rewritten (blaster.rs); the audio and particles are presentation.
        "BeamRepeaterBehaviour" | "AndroidLaserBehaviour" => {}
        "RayCannonBehaviour" | "BeamCannonBehaviour" | "ParticleMachineBehaviour" | "RailgunBehaviour" => {}
        // Awake/OnDestroy (un)subscribe OnContinuousUpdate, whose IsolatedContinuousActivation is unreceived (link refuses
        // a UseWireBehaviour); Start adds a context button; OnDisable hides the light. Update and OnLasered are in laser.rs.
        "LaserReceiverBehaviour" => {}
        // Awake adds the continuous listener (continuous.rs); Start, OnEnable, OnDisable, UpdateDisplay and the gizmo scale in
        // OnWillRenderObject are rewritten (laser.rs, menu.rs); the light colour is rendering. Update is in laser.rs.
        "KeyTriggerBehaviour" => {}
        // Start (ResetTransform) is a pending start (link.rs).
        "ResetTransformBehaviour" => {}
        // Awake, Start, FixedUpdate, Update, Use and Shot are rewritten (fireworks.rs).
        "FireworksBehaviour" => {}
        // Awake, Start and the detail view listener are rewritten (misc.rs; the button in menu.rs).
        "InvisibleOptionInContextMenu" => {}
        // The sprite's _Tint is presentation; Start's button is in menu.rs.
        "EditableTintBehaviour" => {}
        // Update is rewritten (acts.rs).
        "OnlyExistWith" | "OffsetGridBehaviour" | "EggBehaviour" => {}
        // Awake and Update are rewritten (particle_forces.rs): the WindZone feeds ExternalForcesModule.
        "WindControllerBehaviour" => {}
        // Awake and the event are rewritten (acts.rs); OnDestroy only unsubscribes.
        "ActOnDisintegrate" => {}
        // Awake, Update, OnDestroy and the listener are rewritten (timed_gate.rs).
        "ActivationTransformerBehaviour" => {}
        // Awake and OnWillRenderObject are rewritten (sprite_blocks.rs).
        "ResizableSliceBehaviour" => {}
        // Update spins a child sprite: not rewritten, nothing here moves a child Transform yet, so it stays still on screen.
        // FanBehaviour reads CurrentSpeed, so the Fan needs vel rewritten with it.
        "CosmeticRotationBehaviour" => {}
        // FixedUpdate, Update, Use and Start are rewritten (liquid_pump.rs); the rest is the display.
        "LiquidPumpBehaviour" => {}
        // Update and Use are rewritten (liquid_duplicator.rs); Start is the context button (UI).
        "LiquidDuplicatorBehaviour" => {}
        // Its FixedUpdate only throws NotImplementedException, so passingThrough stays false and it never activates (the
        // OnContinuousUpdate listener Awake adds does nothing). Start's buttons are menu.rs's; Update and OnWillRenderObject
        // (the cursor and light) are laser.rs's.
        "LiquidLevelGateBehaviour" => {}
        // Start's button (menu.rs), Shot, ExitShot, Unstabbed and Repair are rewritten (liquid_leak.rs).
        "LiquidContainerLeakingBehaviour" => {}
        // FixedUpdate and OnParticleCollide are rewritten (liquid_leak.rs); Awake is the link.
        "LiquidContainerHoleBehaviour" => {}
        // Awake, Start, Update, OnDisable, the SetTarget joint and the line are rewritten (physics_gun.rs); the sounds are audio.
        "PhysicsGunBehaviour" => {}
        // Use and UseContinuous are rewritten (activator_electrode.rs); Awake and OnDestroy only (un)subscribe an empty listener, Start, Update and the lights are presentation.
        "ActivatorElectrodeBehaviour" => {}
        // Awake sets the physical's delay, Start and OnEnable resume a busy DoLag, Use and OnDisable are rewritten (timed_gate.rs).
        "TimedGateBehaviour" => {}
        // Start, OnEnable, OnDisable, Use and DoLag are rewritten (timed_gate.rs); Awake is the MuteBehaviour link.
        "LagboxBehaviour" => {}
        // Awake, Update and OnDisable are rewritten (timed_gate.rs), Start's buttons in menu.rs; OnEnable and OnWillRenderObject are presentation.
        "GateBehaviour" => {}
        // FixedUpdate, Update, Use and OnDisable are rewritten (hover_thruster.rs); Awake sizes a scratch array, and
        // Start is the context menu (menu.rs) and UpdateActivation (presentation), OnEnable UpdateActivation.
        "HoverThrusterBehaviour" => {}
        // FixedUpdate and Use are rewritten (hover_thruster.rs; calibratedHoverHeights is computed per hover). Awake
        // fills that array; Start is the context menu (menu.rs) and audio, Update, OnEnable and OnDisable the fan and audio.
        "HovercarBehaviour" => {}
        // Start, FixedUpdate, Use, OnEnable and OnDisable are rewritten (winch.rs); Awake's useMotor = true finds it on
        // (link), Update turns the wheel sprite, and the lights, gizmo and audio are presentation.
        // OnFlip is rewritten (winch_update_limits, reached through SelectionController.FlipSelection).
        "WinchBehaviour" => {}
        // Start, Use, FixedUpdate and OnDisable are rewritten (conveyor.rs); the effector's ActOnContacts has no caller.
        "ConveyorBeltBehaviour" | "ConveyorBeltEffector" => {}
        // JointMotorSoundBehaviour.FixedUpdate: audio.rs joint_motor_sound_fixed_update
        // JointMotorSoundBehaviour.constructor: SpeedThreshold = 0.5 (audio.rs)
        // Awake, Start, FixedUpdate and the public methods are rewritten (reactor.rs); the display's text mesh and shader
        // colours are presentation; its alert beep is `reactor_a5_display_update`.
        "ReactorCoreBehaviour" | "ReactorConduitBehaviour" | "ReactorConduitSetBehaviour" | "ReactorMeltdownBehaviour" | "DoAtTime" | "ReactorA5DisplayBehaviour" | "IndustrialLiftBehaviour" | "RandomSoundBehaviour" | "AcidPoolBehaviour" | "VacuumComponent" | "PointAtBehaviour" | "JointMotorSoundBehaviour" | "ReactorA5ArcBehaviour" | "RectangularReverbZone" | "ep" | "PhysicalRopeCreator" => {}
        // Awake (the adopted DistanceJoint2D), Start, FixedUpdate, Update, OnJointBreak2D, OnDestroy and Slice are rewritten (blood_wire.rs); the
        // line, its material and the cable-connect broadcast are presentation.
        "CopperWireBehaviour" => {}
        // FixedUpdate, Use, OnDisable, NextSong and PreviousSong are rewritten (jukebox.rs) and Start's buttons (menu.rs);
        // Start's custom songs stop (jukebox_custom_songs); OnEnable shows the glow (jukebox_on_enable); PlayIndex has no caller.
        "JukeboxBehaviour" => {}
        // Start, Use, Stick, StickTo, Unstick, SetStickDistance and Update are rewritten (coupler.rs); Awake is the link's
        // phys; OnDestroy destroys LineRenderers; the serialisation hooks are save files.
        "AdhesiveCouplerBehaviour" => {}
        // Awake, Update, FixedUpdate, OnEnable, OnDisable, OnDestroy, Connect, ProvideAudioData and GetNextFloat are
        // rewritten (antenna.rs).
        "AntennaReceiverBehaviour" => {}
        // Awake (the vertex count) and Update are rewritten (lightning_tool.rs); the particles, line and flash sprite are
        // presentation; the sounds are in lightning_tool.rs.
        "LightningToolEntityBehaviour" => {}
        // Linked with TeslaCoilBehaviour (tesla_coil.rs): Awake and Update of a bolt spawned on its own.
        "TeslaCoilLightningBoltBehaviour" => {}
        // FixedUpdate is rewritten (pyro_tool.rs); the particles are presentation; the sounds are in pyro_tool.rs.
        "PyroPowerToolEntity" => {}
        // Awake (the bolts), Start, Update and FixedUpdate are rewritten (seismic_explosion.rs); the sounds and the mute are in
        // seismic_explosion.rs; the camera shake is presentation.
        "SeismicChargeExplosionBehaviour" => {}
        // Update and Init are rewritten (seismic_explosion.rs); Awake sets up the LineRenderer (rendering).
        "EnergyVesselLightningBehaviour" => {}
        // Awake (the static set), Start (the clip's instance id), Use and UseContinuous are rewritten (transmitter.rs);
        // OnEnable, OnDisable and SetChannel's text are rewritten (transmitter.rs); Blink's light and sound, and the context menu
        // button, are rendering, audio and UI; OnDestroy leaves the set.
        "TransmitterBehaviour" => {}
        // Start, OnEnable, Use, DefibRoutine and OnDisable are rewritten (resuscitation.rs), the text screen included.
        "DefibrillatorBehaviour" => {}
        // Start, Use, OnEnable (UpdateActivation) and OnDisable are rewritten (resuscitation.rs); the pumping is BloodWireBehaviour's.
        "CardiopulmonaryBypassMachineBehaviour" => {}
        // Use, Update, Awake, Start, OnEnable and OnDisable are rewritten (heart_monitor.rs), the texts included; the line is rendering.
        "HeartMonitorBehaviour" => {}
        // Use, Update and CastSpell are rewritten (excalibur.rs); Awake, the glow and the sounds are presentation.
        "ExcaliburBehaviour" => {}
        // FixedUpdate, SetFloor, EmergencyStop and the joint are rewritten (map_lift.rs); the sounds are in map_lift.rs.
        "MapLiftBehaviour" => {}
        // FixedUpdate, OnDisable and OnDestroy are rewritten (sharp_on_all_sides.rs, soft_connect.rs).
        "SharpOnAllSidesBehaviour" => {}
        // Start sets the static Instance that the bolt's lightning calls Emit on (lightning.rs); it has no other state.
        "SimpleLightningEmitterBehaviour" => {}
        // Awake only subscribes SyncSprite to the parent's OnAfterChange (the sprite is presentation). It draws nothing.
        "RandomSpriteChildBehaviour" => {}
        // Awake (the OnEndOfLife listener), Start and Update are rewritten (ion_bolt.rs).
        "IonBoltBehaviour" => {}
        // AppearWhenMouseNear: level1's marker labels (center_of_gravity.rs); no prefab has one.
        "AppearWhenMouseNear" => {}
        // ResizeHandles: level1's handles (resize_handles.rs), reactivated by ContextMenuBehaviour.ResizeAction; no prefab has one.
        "ResizeHandles" => {}
        // BatchJointSerialisationHelper.Awake: save helper (Broken is a fresh all-false array; only Start and the serialiser read it)
        // BatchJointSerialisationHelper.Start: save helper (destroys the joints Broken marks: none after Awake)
        // BatchJointSerialisationHelper.OnJointBreak2D: save helper (writes Broken, which only the serialiser reads)
        "BatchJointSerialisationHelper" => {}
        // StatLoader.Start: NonSteamStatManager.LoadFromFile("stats") in play::new_game (stats.rs), before Global.Start's.
        "StatLoader" => {}
        // PoolableParticleSystemHelper.Awake: the root ParticleSystem's stopAction = Callback (pool.rs tag_particle_helper).
        "PoolableParticleSystemHelper" => {}
        // PoolableSizeHelper.Awake: localScaleAtStart (the awake arm below, pool.rs poolable_size_helper_awake); a pool
        // object recorded as an item (fnt1_gen's BloodPuff poolable) links here.
        "PoolableSizeHelper" => {}
        // IncreaseStatOnStart.Start is queued (queue_starts, PendingStart::IncreaseStat).
        "IncreaseStatOnStart" => {}
        // Vila.Awake and Vila.OnWillRenderObject, OrganPulseBehaviour.Update and SpookyStuffBehaviour.Start, Tick, StartOfDay and EndOfDay only set
        // material values or play sounds (the presentation notes are at the end of acts.rs).
        "Vila" | "OrganPulseBehaviour" | "SpookyStuffBehaviour" => {}
        // TextMeshProControlReplacer: a world TextMeshPro's Start is queued at link (PendingStart::TextStart, control_replacer_start);
        // one on a screen-space TextMeshProUGUI (a map's UI) only rewrites UI text.
        "TextMeshProControlReplacer" => {}
        // LightSprite.Awake keeps its SpriteRenderer and an empty MaterialPropertyBlock (glow.rs LightSprite; a recording's
        // comes from its ItemScript record).
        "LightSprite" => {}
        name if super::translated::CLASSES.contains(&name) => {}
        _ => {
            let messages = super::lifecycle::LIFECYCLE.iter().find(|(k, _)| *k == class).map_or(&[][..], |(_, m)| *m);
            if !messages.is_empty() {
                unimplemented!("mod-only: {class} ({})", messages.join(", "));
            }
        }
    }
}

impl Game {
    /// Snapshot-loaded limbs: LimbBehaviour.Awake's ShatteredObjectSpriteInitialiser generators from `prefab` (the Human catalog
    /// prefab), each limb's by its same-named object; ConnectTo is the live physical named as the prefab's object.
    pub(crate) fn load_limb_generators_by_name(&mut self, db: &mut AssetDb, prefab: &Prefab) -> Result<(), String> {
        for l in 0..self.limbs.len() {
            if self.limbs[l].ShatteredObjectGenerators.is_some() {
                continue;
            }
            let name = self.physicals[self.limbs[l].PhysicalBehaviour].name.clone();
            let Some(o) = prefab.objects.iter().position(|o| o.name.replace(' ', "_") == name) else { continue };
            let Some((_, _, iv)) = script(prefab, o, "ShatteredObjectSpriteInitialiser") else { continue };
            let file = prefab.objects[o].game_object.file;
            let physicals: Vec<(String, bool)> = self.physicals.iter().map(|p| (p.name.clone(), p.destroyed)).collect();
            let connect_to = |v: &Value| -> Result<Option<usize>, String> {
                let p = v.pptr().ok_or("ConnectTo: not a reference")?;
                if p.is_null() {
                    return Ok(None);
                }
                let &(t, _) = prefab.index.get(&ObjRef { file, path_id: p.path_id }).ok_or("ConnectTo: dangling")?;
                let n = prefab.objects[t].name.replace(' ', "_");
                physicals.iter().position(|(m, d)| *m == n && !d).map(Some).ok_or_else(|| format!("ConnectTo {n}: no such physical"))
            };
            let mut gens = [None; 3];
            for (k, f) in ["BoneGenerator", "FleshGenerator", "SkinGenerator"].into_iter().enumerate() {
                gens[k] = self.load_shatter_generator_from(db, prefab, file, field(iv, f)?, &connect_to)?.map(|_| self.shatter_generators.len() - 1);
            }
            self.limbs[l].ShatteredObjectGenerators = Some(gens);
        }
        Ok(())
    }

    /// The ShatteredObjectGenerator component `reference` points at, as a `shatter_generators` entry (None for a null reference).
    fn load_shatter_generator(&mut self, db: &mut AssetDb, l: &Linker, reference: &Value) -> Result<Option<usize>, String> {
        let Some((o, c)) = self.load_shatter_generator_from(db, l.prefab, l.file, reference, |v| l.physical(v))? else { return Ok(None) };
        self.script_slots.insert(l.inst.objects[o].components[c], super::save_slots::Slot::ShatterGenerators(self.shatter_generators.len() - 1));
        Ok(Some(self.shatter_generators.len() - 1))
    }

    /// The generator `reference` points at in `prefab` (of asset file `file`), pushed to `shatter_generators` with its ConnectTo
    /// through `connect_to`; its (object, component), None for a null reference.
    pub(crate) fn load_shatter_generator_from(&mut self, db: &mut AssetDb, prefab: &Prefab, file: usize, reference: &Value, connect_to: impl Fn(&Value) -> Result<Option<usize>, String>) -> Result<Option<(usize, usize)>, String> {
        let p = reference.pptr().ok_or("ShatteredObjectSpriteInitialiser: not a reference")?;
        if p.is_null() {
            return Ok(None);
        }
        let &(o, c) = prefab.index.get(&ObjRef { file, path_id: p.path_id }).ok_or("ShatteredObjectSpriteInitialiser: a dangling generator")?;
        let Component::Script { class, fields: v, .. } = &prefab.objects[o].components[c].1 else { return Err("ShatteredObjectSpriteInitialiser: not a script".into()) };
        if class != "ShatteredObjectGenerator" {
            return Err(format!("ShatteredObjectSpriteInitialiser: a {class} generator"));
        }
        let sprite_ref = field(v, "Sprite")?.pptr().ok_or("ShatteredObjectGenerator.Sprite")?;
        let sprite_asset = db.resolve(file, sprite_ref).ok_or("ShatteredObjectGenerator.Sprite does not resolve")?;
        let sprite = Sprite::read(db.data(sprite_asset))?;
        // Sprite.uv and texture size only feed the shard material's _AtlasTransform (rendering).
        let int = |name: &str| field(v, name)?.i64().ok_or_else(|| format!("ShatteredObjectGenerator.{name}"));
        let range = field(v, "SoftJointStrengthRange")?;
        let g = shatter::ShatteredObjectGenerator {
            name: string(v, "Name")?,
            PartMass: float(v, "PartMass")?,
            Sprite: shatter::Sprite { id: sprite_asset.path_id as i32, rect_width: sprite.rect[2], rect_height: sprite.rect[3], pixels_per_unit: sprite.pixels_to_units, ..Default::default() },
            PartCount: int("PartCount")? as i32,
            PartSizeRatio: float(v, "PartSizeRatio")?,
            PartsLayer: int("PartsLayer")? as i32,
            PartsPhysicalProperties: 0,
            RandomForceIntensity: float(v, "RandomForceIntensity")?,
            RandomTorqueIntensity: float(v, "RandomTorqueIntensity")?,
            GenerateSoftJointsChance: float(v, "GenerateSoftJointsChance")?,
            GenerateOnStart: field(v, "GenerateOnStart")?.bool().ok_or("ShatteredObjectGenerator.GenerateOnStart")?,
            ConnectTo: connect_to(field(v, "ConnectTo")?)?,
            ConnectToRange: float(v, "ConnectToRange")?,
            SoftJointStrengthRange: [float(range, "x")?, float(range, "y")?],
            Brightness: 1.0,
        };
        let p = field(v, "PartsPhysicalProperties")?.pptr().ok_or("ShatteredObjectGenerator.PartsPhysicalProperties")?;
        let asset = db.resolve(file, p).ok_or("ShatteredObjectGenerator.PartsPhysicalProperties does not resolve")?;
        // Shards add it to `properties` when the first one is made (the recorded arena holds only what a spawned component used).
        let bounds = sprite_bounds(&sprite);
        self.shatter_generators.push(shatter::GeneratorSource { g, properties: read_physical_properties(db, asset)?, sprite: Some(sprite_asset), sprite_bounds: bounds });
        Ok(Some((o, c)))
    }

    /// The instance's scripts as arena entries with their serialized fields and links, in prefab order (the
    /// order GetComponentsInChildren lists them, and the queue 0x14 order within one script class). Limb-owned
    /// components (CirculationBehaviour, ConnectedNodeBehaviour, SkinMaterialHandler, GForceMeasureBehaviour)
    /// share their limb's index. Rigidbody-less Transforms above bodies become `transform_nodes` in the order
    /// the bodies reach them.
    pub fn link_instance(&mut self, db: &mut AssetDb, prefab: &Prefab, inst: &Instance) -> Result<Linked, String> {
        self.link_instance_reusing(db, prefab, inst, Reuse::default())
    }

    /// `link_instance` over arena entries that already exist (a recording's restored PhysicalBehaviours, limbs, grips and
    /// hinges): the objects `reuse` names keep those slots, and only the others get new entries.
    pub fn link_instance_reusing(&mut self, db: &mut AssetDb, prefab: &Prefab, inst: &Instance, reuse: Reuse) -> Result<Linked, String> {
        let file = prefab.objects[0].game_object.file;
        let mut l = Linker { prefab, inst, file, arenas: Arenas { physical: reuse.physical.clone(), limb: reuse.limb.clone(), grip: reuse.grip.clone(), hinge: reuse.hinge.clone(), node: HashMap::new() }, under: self.link_under.take() };
        let n = prefab.objects.len();
        let embedded_from = self.embedded_ps.len();
        let person = script(prefab, 0, "PersonBehaviour").map(|_| self.people.len());

        // TransformChain: each Rigidbody2D's parents up to the root (an inactive one's too: it has no body yet), the
        // first to reach one listing it.
        let mut body_parents = std::collections::BTreeSet::new();
        for o in (0..n).filter(|&o| prefab.objects[o].components.iter().any(|(_, x)| matches!(x, Component::Rigidbody2D(_)))) {
            let mut p = inst.objects[o].parent;
            while let Some(t) = p {
                // A static body never moves in the simulation, so its Transform serves as a plain node (the
                // Default map's walls under Root); a moving body's node follows its physical.
                let static_body = prefab.objects[t].components.iter().any(|(_, x)| matches!(x, Component::Rigidbody2D(rb) if rb.body_type == 2));
                if inst.bodies.contains_key(&t) && !static_body {
                    body_parents.insert(t);
                }
                if l.arenas.node.contains_key(&t) {
                    break;
                }
                l.arenas.node.insert(t, self.transform_nodes.len());
                let transform = prefab.objects[t].components.iter().position(|(_, c)| matches!(c, Component::Transform(_))).ok_or("an object without a Transform")?;
                self.transform_nodes.push(TransformNode { id: inst.objects[t].components[transform], trs: inst.objects[t].trs, parent: None });
                p = inst.objects[t].parent;
            }
        }
        // A Rigidbody2D without a PhysicalBehaviour: its own Transform is a node its body writes.
        let bare_bodies: Vec<usize> = (0..n).filter(|&o| script(prefab, o, "PhysicalBehaviour").is_none() && prefab.objects[o].components.iter().any(|(_, x)| matches!(x, Component::Rigidbody2D(_)))).collect();
        for &o in &bare_bodies {
            if !l.arenas.node.contains_key(&o) {
                l.arenas.node.insert(o, self.transform_nodes.len());
                let transform = prefab.objects[o].components.iter().position(|(_, c)| matches!(c, Component::Transform(_))).ok_or("an object without a Transform")?;
                self.transform_nodes.push(TransformNode { id: inst.objects[o].components[transform], trs: inst.objects[o].trs, parent: None });
            }
        }
        for (&o, &i) in &l.arenas.node {
            self.transform_nodes[i].parent = inst.objects[o].parent.map(|t| l.arenas.node[&t]);
        }

        // Arena slots first, so every reference resolves.
        let mut next = (self.physicals.len(), self.limbs.len(), self.grips.len(), self.hinges.len());
        for o in 0..n {
            if script(prefab, o, "PhysicalBehaviour").is_some() && !reuse.physical.contains_key(&o) {
                l.arenas.physical.insert(o, next.0);
                next.0 += 1;
            }
            if script(prefab, o, "LimbBehaviour").is_some() && !reuse.limb.contains_key(&o) {
                l.arenas.limb.insert(o, next.1);
                next.1 += 1;
            }
            if script(prefab, o, "GripBehaviour").is_some() && !reuse.grip.contains_key(&o) {
                l.arenas.grip.insert(o, next.2);
                next.2 += 1;
            }
            for (c, (_, comp)) in prefab.objects[o].components.iter().enumerate() {
                match comp {
                    Component::Joint2D(j) if matches!(j.kind, JointKind::Hinge { .. }) && !reuse.hinge.contains_key(&(o, c)) => {
                        l.arenas.hinge.insert((o, c), next.3);
                        next.3 += 1;
                    }
                    _ => {}
                }
            }
        }
        for t in body_parents {
            let p = *l.arenas.physical.get(&t).ok_or_else(|| format!("{}: a body under a Rigidbody2D without a PhysicalBehaviour", prefab.objects[t].name))?;
            // A reused (recorded) body keeps the node the recording's loader gave it: the clone's children hang from that one.
            match self.node_bodies.iter().find_map(|(&n, &q)| (q == p).then_some(n)) {
                Some(n0) => {
                    let n = l.arenas.node.insert(t, n0).unwrap();
                    for x in self.transform_nodes.iter_mut().filter(|x| x.parent == Some(n)) {
                        x.parent = Some(n0);
                    }
                    for x in self.physicals.iter_mut().filter(|x| x.parent == Some(n)) {
                        x.parent = Some(n0);
                    }
                }
                None => {
                    self.node_bodies.insert(l.arenas.node[&t], p);
                }
            }
        }
        let mut node_rigidbodies = HashMap::new();
        for &o in &bare_bodies {
            let Some(Component::Rigidbody2D(rb)) = prefab.objects[o].components.iter().map(|(_, x)| x).find(|x| matches!(x, Component::Rigidbody2D(_))) else { unreachable!() };
            // An inactive object's Rigidbody2D has no body (m_Body null) until it is activated.
            let (body, detached) = match inst.bodies.get(&o) {
                Some(&body) => (body, None),
                None if !inst.objects[o].active => (usize::MAX, Some(crate::b2::world::RigidbodyMass { use_auto_mass: rb.use_auto_mass, mass: rb.mass, inertia: None, center_of_mass: None })),
                None => unreachable!("{}: an active Rigidbody2D without a body", prefab.objects[o].name),
            };
            let component = inst.objects[o].components[prefab.objects[o].components.iter().position(|(_, x)| matches!(x, Component::Rigidbody2D(_))).unwrap()];
            let rigidbody = Rigidbody { body, bodyType: rb.body_type, collisionDetectionMode: rb.collision_detection, interpolation: rb.interpolate, movement: created_movement(&inst, o), detached, component, simulated: rb.simulated, detached_props: [rb.linear_drag, rb.angular_drag, rb.gravity_scale], constraints: rb.constraints };
            self.rigidbody_hierarchy_changed |= detached.is_none() && rb.body_type != 2;
            let mut driven_by = None;
            if rb.body_type != 2 && detached.is_none() {
                let mut up = inst.objects[o].parent;
                while let Some(t) = up {
                    if prefab.objects[t].components.iter().any(|(_, x)| matches!(x, Component::Rigidbody2D(_))) {
                        driven_by = Some(*inst.bodies.get(&t).ok_or_else(|| format!("{}: driven by a Rigidbody2D without a body", prefab.objects[o].name))?);
                        break;
                    }
                    up = inst.objects[t].parent;
                }
            }
            node_rigidbodies.insert(o, self.node_rigidbodies.len());
            self.node_rigidbodies.push(NodeBody { node: l.arenas.node[&o], rigidbody, driven_by });
        }
        // Soap's Clean walks a hit collider's ancestors for DecalControllers; the non-physical ones are not rewritten.
        for o in 0..n {
            for (c, (_, comp)) in prefab.objects[o].components.iter().enumerate() {
                if !matches!(comp, Component::Collider2D(_)) {
                    continue;
                }
                let (mut up, mut decal) = (Some(o), false);
                while let Some(t) = up {
                    if !l.arenas.physical.contains_key(&t) && prefab.objects[t].components.iter().any(|(_, x)| matches!(x, Component::Script { class, .. } if class == "DecalControllerBehaviour")) {
                        decal = true;
                        break;
                    }
                    up = inst.objects[t].parent;
                }
                if decal {
                    self.decal_chain_colliders.insert(inst.objects[o].components[c]);
                }
            }
        }
        // Colliders and sprites without a PhysicalBehaviour on their GameObject: the Transforms down from the
        // nearest physical or node.
        let anchor_of = |o: usize| l.anchor_chain(o);
        for o in (0..n).filter(|&o| !l.arenas.physical.contains_key(&o)) {
            for (c, (_, comp)) in prefab.objects[o].components.iter().enumerate() {
                match comp {
                    Component::Collider2D(_) => {
                        let (anchor, chain) = anchor_of(o);
                        let scripts = prefab.objects[o].components.iter().filter_map(|(_, x)| if let Component::Script { class, .. } = x { Some(class.clone()) } else { None }).collect();
                        let wi = script(prefab, o, "WaterImpactBehaviour");
                        if let Some((k, _, _)) = wi {
                            self.water_impact_colliders.insert(inst.objects[o].components[k], inst.objects[o].components[c]);
                        }
                        let water_impact = wi.and_then(|(_, _, s)| s.get("Splash")).and_then(|x| x.pptr()).and_then(|pp| db.resolve(file, pp));
                        self.bare_colliders.insert(inst.objects[o].components[c], BareCollider { anchor, chain, scripts, water_impact, active_self: prefab.objects[o].active, root: (inst.objects[0].game_object, inst.objects[0].components[0]) });
                    }
                    Component::SpriteRenderer(r) => {
                        let Some(sprite) = db.resolve(file, r.sprite) else { continue };
                        let (anchor, chain) = anchor_of(o);
                        let visible = inst.objects[o].active;
                        let bounds = renderer_bounds(&Sprite::read(db.data(sprite))?, r);
                        self.renderers.push(Renderer { anchor, chain, sprite: (sprite, r.color, r.sorting_order), layer: r.sorting_layer_id, game_layer: prefab.objects[o].layer as i32, material: r.materials.first().and_then(|&m| db.resolve(file, m)), id: inst.objects[o].components[c], draw: ([r.flip_x, r.flip_y], r.draw_mode, r.size, r.adaptive_mode_threshold, r.sprite_tile_mode == 1), visible, enabled: r.enabled, bounds });
                    }
                    _ => {}
                }
            }
        }
        // Every AudioSource (AudioSource::AwakeFromLoad, AddToManager: a live one with playOnAwake plays).
        for o in 0..n {
            for (c, (_, comp)) in prefab.objects[o].components.iter().enumerate() {
                if let Component::Script { fields: Value::Struct(fs), .. } = comp {
                    let id = inst.objects[o].components[c];
                    self.audio.register_clip_fields(db, file, id, &Value::Struct(fs.clone()))?;
                    self.audio.register_event_clips(db, file, &Value::Struct(fs.clone()))?;
                    // Each UnityEvent field's persistent calls, run by `invoke_event`.
                    for (name, ev) in fs {
                        if ev.get("m_PersistentCalls").is_some() {
                            let Component::Script { class, .. } = comp else { unreachable!() };
                            let calls = super::unity_event::calls(prefab, file, inst, ev, &format!("{class}.{name}"))?;
                            if !calls.is_empty() {
                                self.unity_events.insert((id, name.clone()), calls);
                            }
                        }
                    }
                    // AudioSource fields into this clone.
                    let source = |p: crate::assets::classes::PPtr| {
                        (p.file == 0 && !p.is_null()).then(|| prefab.index.get(&ObjRef { file, path_id: p.path_id })).flatten().filter(|&&(to, tc)| tc != usize::MAX && matches!(prefab.objects[to].components[tc].1, Component::AudioSource { .. })).map(|&(to, tc)| inst.objects[to].components[tc])
                    };
                    for (name, v) in fs {
                        let found: Vec<Option<i32>> = match v {
                            Value::PPtr(p) => vec![source(*p)],
                            Value::Array(a) if a.first().is_some_and(|x| x.pptr().is_some()) => a.iter().map(|x| x.pptr().and_then(source)).collect(),
                            _ => continue,
                        };
                        if found.iter().any(Option::is_some) {
                            self.audio.source_fields.insert((id, name.clone()), found);
                        }
                    }
                }
                if let Component::AudioSource { source, clip, group } = comp {
                    let (anchor, chain) = anchor_of(o);
                    let clip = super::audio::clip_ref(db, &mut self.audio.clip_cache, *clip)?;
                    let ancestors = std::iter::successors(Some(o), |&x| inst.objects[x].parent).map(|x| inst.objects[x].components[0]).collect();
                    let s = super::audio::AudioSource::from_component(source, clip, group.as_deref(), anchor, chain, ancestors, inst.objects[o].active);
                    self.audio.add(inst.objects[o].components[c], s);
                }
            }
            let here: Vec<i32> = prefab.objects[o].components.iter().enumerate().filter(|(_, (_, k))| matches!(k, Component::AudioSource { .. })).map(|(c, _)| inst.objects[o].components[c]).collect();
            if !here.is_empty() {
                for &id in &inst.objects[o].components {
                    self.audio.object_sources.insert(id, here.clone());
                }
            }
        }
        for o in (0..n).filter(|&o| inst.objects[o].active) {
            if let Some((c, _, _)) = script(prefab, o, "ExplosionSoundBehviour") {
                self.explosion_sound_awake(inst.objects[o].components[c]);
            }
        }
        // TMPro.TextMeshPro: the text, its RectTransform and its MeshRenderer (world_text.rs).
        for o in 0..n {
            for (c, (_, comp)) in prefab.objects[o].components.iter().enumerate() {
                let Component::Script { class, enabled, fields: v, .. } = comp else { continue };
                if class != "TextMeshPro" {
                    continue;
                }
                if l.arenas.physical.contains_key(&o) {
                    unimplemented!("mod-only: {}: a TextMeshPro on a PhysicalBehaviour's object", prefab.objects[o].name);
                }
                let (anchor, chain) = anchor_of(o);
                self.add_world_text(db, file, prefab, o, v, *enabled, inst.objects[o].components[c], (o, inst.objects[0].components[0]), anchor, chain, inst.objects[o].active)?;
            }
        }
        for class in ["CirculationBehaviour", "ConnectedNodeBehaviour", "SkinMaterialHandler"] {
            for o in 0..n {
                if (script(prefab, o, class).is_some()) != l.arenas.limb.contains_key(&o) {
                    return Err(format!("{}: {class} without its LimbBehaviour (or the reverse)", prefab.objects[o].name));
                }
            }
        }

        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "PhysicalBehaviour") else { continue };
            let mut p = Physical { name: prefab.objects[o].name.clone(), id: inst.objects[o].components[c], ..Default::default() };
            load_values(p.fields(), v)?;
            p.SoundVolumeBoost = field(v, "SoundVolumeBoost")?.f32().ok_or("PhysicalBehaviour.SoundVolumeBoost")?;
            p.enabled = enabled;
            p.active = inst.objects[o].active;
            // GameObject.isStatic: player data carries no static editor flags.
            p.isStatic = false;
            p.colliders = prefab.objects[o].components.iter().enumerate().filter(|(_, (_, x))| matches!(x, Component::Collider2D(_))).map(|(k, _)| inst.objects[o].components[k]).collect();
            p.collider_defs = prefab.objects[o].components.iter().filter_map(|(_, x)| match x {
                Component::Collider2D(c) => Some(c.clone()),
                _ => None,
            }).collect();
            p.all_colliders = p.colliders.clone();
            p.all_colliders.sort_by(|a, b| b.cmp(a));
            p.transform_id = inst.objects[o].components[0];
            p.BulletPenetration = field(v, "BulletPenetration")?.bool().ok_or("BulletPenetration")?;
            // 1.28.45's SelectionPriority, PreventGripping, PauseDragOverride and SlashColliders (absent from 1.28.41's prefabs).
            if crate::assets::il::is_1_28_45() {
                p.SelectionPriority = field(v, "SelectionPriority")?.bool().ok_or("SelectionPriority")?;
                p.PreventGripping = field(v, "PreventGripping")?.bool().ok_or("PreventGripping")?;
                p.PauseDragOverride = field(v, "PauseDragOverride")?.bool().ok_or("PauseDragOverride")?;
                let slash = array(v, "SlashColliders")?.iter().map(|x| {
                    let r = x.pptr().ok_or("SlashColliders: not a reference")?;
                    if r.is_null() {
                        return Ok(0);
                    }
                    let &(so, sc) = prefab.index.get(&ObjRef { file, path_id: r.path_id }).filter(|_| r.file == 0).ok_or("PhysicalBehaviour.SlashColliders outside the prefab")?;
                    if !matches!(prefab.objects[so].components.get(sc), Some((_, Component::Collider2D(_)))) {
                        return Err("PhysicalBehaviour.SlashColliders: not a Collider2D".to_string());
                    }
                    Ok(inst.objects[so].components[sc])
                }).collect::<Result<Vec<i32>, String>>()?;
                p.SlashColliders = Some(slash);
            }
            p.Resizable = field(v, "Resizable")?.bool().ok_or("Resizable")?;
            p.ReflectsLasers = field(v, "ReflectsLasers")?.bool().ok_or("ReflectsLasers")?;
            p.AbsorbsLasers = field(v, "AbsorbsLasers")?.bool().ok_or("AbsorbsLasers")?;
            p.BulletDeflectChanceOverride = float(v, "BulletDeflectChanceOverride")?;
            p.OverrideShotSounds = array(v, "OverrideShotSounds")?.len();
            // Start adds it to Global.PhysicalObjectsInWorld (an inactive object never starts).
            p.in_world = false;
            p.has_optout = script(prefab, o, "Optout").is_some();
            p.Properties = match db.resolve(file, field(v, "Properties")?.pptr().ok_or("Properties")?) {
                Some(asset) => self.physical_properties(db, asset)?,
                None => unimplemented!("mod-only: {}: PhysicalBehaviour without Properties", p.name),
            };
            let Some(Component::Rigidbody2D(rb)) = prefab.objects[o].components.iter().map(|(_, x)| x).find(|x| matches!(x, Component::Rigidbody2D(_))) else {
                return Err(format!("{}: PhysicalBehaviour without a Rigidbody2D", p.name));
            };
            // An inactive object's Rigidbody2D has no body (m_Body null) until it is activated.
            let (body, detached) = match inst.bodies.get(&o) {
                Some(&body) => (body, None),
                None if !p.active => (usize::MAX, Some(crate::b2::world::RigidbodyMass { use_auto_mass: rb.use_auto_mass, mass: rb.mass, inertia: None, center_of_mass: None })),
                None => unreachable!("{}: an active Rigidbody2D without a body", prefab.objects[o].name),
            };
            let component = inst.objects[o].components[prefab.objects[o].components.iter().position(|(_, x)| matches!(x, Component::Rigidbody2D(_))).unwrap()];
            p.rigidbody = Rigidbody { body, bodyType: rb.body_type, collisionDetectionMode: rb.collision_detection, interpolation: rb.interpolate, movement: created_movement(&inst, o), detached, component, simulated: rb.simulated, detached_props: [rb.linear_drag, rb.angular_drag, rb.gravity_scale], constraints: rb.constraints };
            // Rigidbody2D::Create set the scene's m_RigidbodyHierarchyChanged.
            self.rigidbody_hierarchy_changed |= detached.is_none() && rb.body_type != 2;
            p.transform = pose(inst, o);
            p.parent = inst.objects[o].parent.map(|t| l.arenas.node[&t]);
            // Rigidbody2D::Create's CheckForDrivenByParentRigidbody.
            if rb.body_type != 2 && detached.is_none() {
                let mut up = inst.objects[o].parent;
                while let Some(t) = up {
                    if prefab.objects[t].components.iter().any(|(_, x)| matches!(x, Component::Rigidbody2D(_))) {
                        p.driven_by = Some(*inst.bodies.get(&t).ok_or_else(|| format!("{}: driven by a Rigidbody2D without a body", p.name))?);
                        break;
                    }
                    up = inst.objects[t].parent;
                }
            }
            p.LocalColliderGridPoints = Some(array(v, "LocalColliderGridPoints")?.iter().map(floats::<2>).collect::<Result<_, _>>()?);
            p.HoldingPositions = array(v, "HoldingPositions")?.iter().map(floats::<3>).collect::<Result<_, _>>()?;
            // Awake's GetComponent<LocalFirePropagationMap>() and Start's GetComponents<DecalControllerBehaviour>().
            p.has_fire_map = script(prefab, o, "LocalFirePropagationMap").is_some();
            p.prefab_decal_controllers = prefab.objects[o].components.iter().filter(|(_, x)| matches!(x, Component::Script { class, .. } if class == "DecalControllerBehaviour")).count();
            p.audio_time_scale = script(prefab, o, "AudioSourceTimeScaleBehaviour").is_some();
            p.audioSource = prefab.objects[o].components.iter().any(|(_, x)| matches!(x, Component::AudioSource { .. }));
            p.MaximumStabVictims = field(v, "MaximumStabVictims")?.i64().ok_or("MaximumStabVictims")? as i32;
            p.LooseEndSize = float(v, "LooseEndSize")?;
            p.StabReleaseSpeedModifier = float(v, "StabReleaseSpeedModifier")?;
            p.StabWoundSizeMultiplier = float(v, "StabWoundSizeMultiplier")?;
            p.ActivationPropagationDelay = float(v, "ActivationPropagationDelay")?;
            p.SendUserPropagation = field(v, "SendUserPropagation")?.bool().ok_or("SendUserPropagation")?;
            p.OverrideImpactSounds = array(v, "OverrideImpactSounds")?.len();
            let renderer = prefab.objects[o].components.iter().find_map(|(_, x)| match x {
                Component::SpriteRenderer(r) => Some(r),
                _ => None,
            });
            if let Some(r) = renderer {
                let sprite = db.resolve(file, r.sprite).ok_or("a SpriteRenderer without a sprite")?;
                p.sprite_bounds = Some(renderer_bounds(&Sprite::read(db.data(sprite))?, r));
                p.sprite = Some((sprite, r.color, r.sorting_order));
                p.sprite_enabled = r.enabled;
                p.sprite_id = prefab.objects[o].components.iter().position(|(_, x)| matches!(x, Component::SpriteRenderer(_))).map_or(0, |k| inst.objects[o].components[k]);
                p.sprite_layer = r.sorting_layer_id;
                p.sprite_game_layer = prefab.objects[o].layer as i32;
                p.sprite_material = r.materials.first().and_then(|&m| db.resolve(file, m));
                p.sprite_draw = ([r.flip_x, r.flip_y], r.draw_mode, r.size, r.adaptive_mode_threshold, r.sprite_tile_mode == 1);
            }
            // Scripts on this object whose context menu buttons are not rewritten yet: one marker each.
            for (_, comp) in &prefab.objects[o].components {
                if let Component::Script { class, .. } = comp {
                    if let Some(&c) = super::menu::NOT_YET.iter().find(|&&c| c == class) {
                        p.menu.push(super::menu::MenuButton::new(c, c, "Not rewritten yet", super::menu::MenuAction::NotYet(c)));
                    }
                }
            }
            match reuse.physical.get(&o) {
                Some(&k) => fill_from_prefab(&mut self.physicals[k], p),
                None => self.physicals.push(p),
            }
        }

        for o in 0..n {
            for (c, (_, comp)) in prefab.objects[o].components.iter().enumerate() {
                let Component::Joint2D(j) = comp else { continue };
                let JointKind::Hinge { use_motor, motor_speed, max_motor_torque, use_limits, lower, upper } = j.kind else { continue };
                if reuse.hinge.contains_key(&(o, c)) {
                    continue;
                }
                let connected = l.object_of(j.connected_body)?;
                let h = Hinge {
                    joint: inst.joints.get(&(o, c)).copied(),
                    useMotor: use_motor,
                    motor: [motor_speed, max_motor_torque],
                    useLimits: use_limits,
                    limits: [lower, upper],
                    breakForce: j.break_force,
                    breakTorque: j.break_torque,
                    connected: connected.map(|b| inst.bodies.get(&b).copied().ok_or("a hinge to an object without a Rigidbody2D")).transpose()?,
                    anchor: j.anchor,
                    connectedAnchor: inst.hinges.get(&(o, c)).map_or(j.connected_anchor, |s| s.connected_anchor),
                    enabled: j.enabled,
                    destroyed: false,
                    destroy_at: None,
                };
                // The create put the joint in m_JointBreakLimits as BreakJoint::Anchored(slot): it becomes Hinge(h), in place.
                if let Some(slot) = inst.hinges.get(&(o, c)).and_then(|s| s.slot) {
                    let anchored = BreakJoint::Anchored(slot);
                    for k in self.joint_break_limits.iter_mut().filter(|k| **k == anchored) {
                        *k = BreakJoint::Hinge(self.hinges.len());
                    }
                    self.anchored_joints[slot].joint = None;
                    self.anchored_joints[slot].hinge = Some(self.hinges.len());
                }
                self.hinges.push(h);
            }
        }

        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "LimbBehaviour") else { continue };
            if reuse.limb.contains_key(&o) {
                continue;
            }
            let mut b = Limb { name: prefab.objects[o].name.clone(), id: inst.objects[o].components[c], ..Default::default() };
            load_values(b.fields(), v)?;
            b.enabled = enabled;
            // Awake's GetComponent<PhysicalBehaviour/HingeJoint2D/GripBehaviour>() (the first of each on the object),
            // and PersonBehaviour.Awake's limb.Person = this.
            b.PhysicalBehaviour = *l.arenas.physical.get(&o).ok_or_else(|| format!("{}: LimbBehaviour without PhysicalBehaviour", b.name))?;
            b.Person = person.ok_or_else(|| format!("{}: a LimbBehaviour outside a Person", b.name))?;
            b.Joint = (0..prefab.objects[o].components.len()).find_map(|k| l.arenas.hinge.get(&(o, k)).copied());
            b.GripBehaviour = l.arenas.grip.get(&o).copied();
            b.ConnectedLimbs = l.limbs(v, "ConnectedLimbs")?;
            b.KillShotParticles = l.object(field(v, "KillShotParticles")?)?.is_some();
            b.NearestLimbToBrain = l.limb(field(v, "NearestLimbToBrain")?)?;
            b.StrengthMultipliers = array(v, "StrengthMultipliers")?.iter().map(|x| x.f32().ok_or("StrengthMultipliers")).collect::<Result<_, _>>()?;
            b.SpeciesIdentity = string(v, "SpeciesIdentity")?;
            if self.limb_status_look.is_none() {
                self.limb_status_look = limb_status_look(db, file, field(v, "LimbStatus")?)?;
            }
            b.BloodLiquidType = string(v, "BloodLiquidType")?;
            // A null BloodDecal (the Android's limbs) stays the empty name: see Game::limb_blood_decal.
            let decal = field(v, "BloodDecal")?.pptr().ok_or("BloodDecal")?;
            b.BloodDecal = match db.resolve(file, decal) {
                Some(decal) => {
                    let decal = self.load_decal_descriptor(db, decal)?;
                    self.decal_descriptors[decal].name.clone()
                }
                None if decal.is_null() => String::new(),
                None => return Err("LimbBehaviour: a BloodDecal that does not resolve".into()),
            };
            // extras: a new species' blood stains in its own colour (src/extras_hook.rs).
            crate::extras_hook::blood_decal(self, &b.SpeciesIdentity, &mut b.BloodDecal);
            self.vital_parts.insert(self.limbs.len(), array(v, "VitalParts")?.iter().map(bounds).collect::<Result<_, _>>()?);
            // Awake's GetComponent<ShatteredObjectSpriteInitialiser>() and its three generators.
            b.ShatteredObjectGenerators = match script(prefab, o, "ShatteredObjectSpriteInitialiser") {
                None => None,
                Some((_, _, iv)) => {
                    let mut gens = [None; 3];
                    for (k, name) in ["BoneGenerator", "FleshGenerator", "SkinGenerator"].into_iter().enumerate() {
                        gens[k] = self.load_shatter_generator(db, &l, field(iv, name)?)?;
                    }
                    Some(gens)
                }
            };
            // Awake's GetComponent<GoreStringBehaviour>().
            b.goreString = false;
            if let Some((gc, gore_enabled, gv)) = script(prefab, o, "GoreStringBehaviour") {
                b.goreString = true;
                b.goreStringEnabled = gore_enabled;
                // GoreStringBehaviour.Joint is not serialized: Start makes it (CreateJointOnStart).
                b.goreStringJoint = false;
                b.gore = GoreString {
                    Other: l.limb(field(gv, "Other")?)?,
                    JointDampening: float(gv, "JointDampening")?,
                    JointFreq: float(gv, "JointFreq")?,
                    JointBreakThreshold: float(gv, "JointBreakThreshold")?,
                    InvincibilityTime: float(gv, "InvincibilityTime")?,
                    OriginLine: line(field(gv, "OriginLine")?)?,
                    OtherLine: line(field(gv, "OtherLine")?)?,
                    LineWidthMultiplier: float(gv, "LineWidthMultiplier")?,
                    RenderOrder: field(gv, "RenderOrder")?.i64().ok_or("RenderOrder")? as i32,
                    TissueColour: floats(field(gv, "TissueColour")?)?,
                    ColourSource: l.limb(field(gv, "ColourSource")?)?,
                    HadJointWhenSerialised: field(gv, "HadJointWhenSerialised")?.bool().ok_or("HadJointWhenSerialised")?,
                    TissueSnapClips: array(gv, "TissueSnapClips")?.len(),
                    AudioSource: l.object(field(gv, "AudioSource")?)?.is_some(),
                    id: inst.objects[o].components[gc],
                    ..Default::default()
                };
                b.gore.awake();
            }
            self.limbs.push(b);
        }

        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "GripBehaviour") else { continue };
            if reuse.grip.contains_key(&o) {
                continue;
            }
            let mut g = Grip::default();
            load_values(g.fields(), v)?;
            g.id = inst.objects[o].components[c];
            // GripBehaviour.Awake: PhysicalBehaviour = GetComponent<PhysicalBehaviour>() (the OnChangePause listener is Game::change_pause's grip loop).
            g.PhysicalBehaviour = *l.arenas.physical.get(&o).ok_or("GripBehaviour without PhysicalBehaviour")?;
            g.CurrentlyHolding = l.physical(field(v, "CurrentlyHolding")?)?;
            self.grips.push(g);
        }

        for o in 0..n {
            let Some((_, enabled, v)) = script(prefab, o, "CirculationBehaviour") else { continue };
            // OnDistributionChange runs keyed by the LimbBehaviour's id (`container_id`).
            let calls = super::unity_event::calls(prefab, file, inst, field(v, "OnDistributionChange")?, "CirculationBehaviour.OnDistributionChange")?;
            if !calls.is_empty() {
                let (k, _, _) = script(prefab, o, "LimbBehaviour").ok_or("CirculationBehaviour without a LimbBehaviour")?;
                self.unity_events.insert((inst.objects[o].components[k], "OnDistributionChange".into()), calls);
            }
            let mut c = Circulation::default();
            load_values(c.fields(), v)?;
            c.enabled = enabled;
            c.Source = l.limb(field(v, "Source")?)?;
            c.PushesTo = l.limbs(v, "PushesTo")?;
            c.LocalArteryRects = Some(array(v, "LocalArteryRects")?.iter().map(bounds).collect::<Result<_, _>>()?);
            self.circulations.push(c);
        }
        for o in 0..n {
            let Some((_, _, v)) = script(prefab, o, "ConnectedNodeBehaviour") else { continue };
            let mut d = Node::default();
            load_values(d.fields(), v)?;
            d.Connections = array(v, "Connections")?.iter().map(|x| l.limb(x)).collect::<Result<_, _>>()?;
            self.nodes.push(d);
        }
        for o in 0..n {
            let Some((_, _, v)) = script(prefab, o, "SkinMaterialHandler") else { continue };
            let mut k = Skin::default();
            load_values(k.fields(), v)?;
            k.adjacentLimbs = l.limbs(v, "adjacentLimbs")?;
            self.skins.push(k);
        }
        // LimbBehaviour.Awake's GetOrAddComponent<GForceMeasureBehaviour>() adds the ones the prefab lacks; the
        // arena keeps the limb index only if the prefab has all or none.
        // Stored by ascending instance id, as a recording lists them.
        let mut map_lights = HashMap::new();
        let mut lights: Vec<(i32, usize)> = (0..n).filter_map(|o| script(prefab, o, "MapLightBehaviour").map(|(c, _, _)| (inst.objects[o].components[c], o))).collect();
        lights.sort();
        for (_, o) in lights {
            let (_, _, v) = script(prefab, o, "MapLightBehaviour").unwrap();
            let mut m = MapLight::default();
            load_values(m.fields(), v)?;
            // A serialized array is never null.
            m.switches = Some(array(v, "Switches")?.len());
            // OnGroup: the GameObject's Transform (its first component), which SetActive reaches through the instance.
            let group = l.object(field(v, "OnGroup")?)?;
            m.on_group = group.map(|go| inst.objects[go].components[0]);
            m.on_group_active = group.is_some_and(|go| inst.objects[go].active);
            m.hasAudio = l.object(field(v, "AudioSource")?)?.is_some();
            m.id = inst.objects[o].components[script(prefab, o, "MapLightBehaviour").unwrap().0];
            if l.object(field(v, "OffGroup")?)?.is_some() {
                unimplemented!("mod-only: MapLightBehaviour.UpdateActivated with an OffGroup");
            }
            m.SetPropertyBlock = field(v, "SetPropertyBlock")?.bool().ok_or("MapLightBehaviour.SetPropertyBlock")?;
            m.spriteRenderer = self.sprite_at(&l, o);
            map_lights.insert(o, self.map_lights.len());
            self.map_lights.push(m);
            self.map_light_awake(self.map_lights.len() - 1);
        }
        let mut waters = HashMap::new();
        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "WaterBehaviour") else { continue };
            let component = |name: &str| -> Result<(usize, usize), String> {
                let pp = field(v, name)?.pptr().ok_or_else(|| name.to_string())?;
                prefab.index.get(&ObjRef { file, path_id: pp.path_id }).copied().filter(|_| !pp.is_null() && pp.file == 0).ok_or_else(|| format!("WaterBehaviour.{name} outside the prefab or null"))
            };
            let mut w = Water::default();
            load_values(w.fields(), v)?;
            (w.id, w.object, w.component, w.enabled, w.active) = (inst.objects[o].components[c], o, c, enabled, inst.objects[o].active);
            w.chain = inst.chain(o);
            let (to, tc) = component("Trigger")?;
            w.trigger = inst.objects[to].components[tc];
            w.trigger_chain = inst.chain(to);
            w.trigger_fixtures = inst.fixtures.get(&(to, tc)).cloned().unwrap_or_default();
            w.ice_object = component("Ice")?.0;
            if let Some(k) = prefab.objects[o].components.iter().position(|(_, x)| matches!(x, Component::SpriteRenderer(_))) {
                let rid = inst.objects[o].components[k];
                w.renderer = self.renderers.iter().position(|r| r.id == rid);
            }
            w.ice = *l.arenas.physical.get(&w.ice_object).ok_or("WaterBehaviour.Ice without a PhysicalBehaviour")?;
            w.boiling_sound = l.object(field(v, "BoilingWaterSound")?)?.ok_or("WaterBehaviour.BoilingWaterSound is null")?;
            for x in array(v, "ObjectsToBeActiveIfWater")? {
                w.objects_to_be_active.push(l.object(x)?.ok_or("a null ObjectsToBeActiveIfWater entry")?);
            }
            if let Some(r) = field(v, "Splash")?.pptr().and_then(|pp| db.resolve(file, pp)) {
                w.splash_pool = Some(self.splash_pool_for(db, r)?);
            }
            w.waterSizzles = db.resources_in("Audio/water_sizzle", 83)?.len();
            waters.insert(o, self.waters.len());
            self.waters.push(w);
        }
        // ActOnDestroy: Event runs through `invoke_event`; DestroyWith is filled at run time.
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "ActOnDestroy") else { continue };
            let mut a = super::destroyable::ActOnDestroy { id: inst.objects[o].components[c], ..Default::default() };
            for g in array(v, "DestroyWith")? {
                if let Some(x) = l.object(g)? {
                    a.destroy_with.push(inst.objects[x].components[0]);
                }
            }
            self.act_on_destroy.insert(inst.objects[o].components[0], a);
        }
        for o in 0..n {
            if let Some((c, _, v)) = script(prefab, o, "InstantiateObjectBehaviour") {
                let mut f = super::destroyable::InstantiateObject::default();
                load_values(f.fields(), v)?;
                let p = field(v, "Prefab")?.pptr().ok_or("Prefab")?;
                let source = if p.is_null() { None } else { Some(db.resolve(file, p).ok_or("InstantiateObjectBehaviour.Prefab outside the data")?) };
                self.instantiate_objects.insert(inst.objects[o].components[c], (source, f));
            }
            // DoAtTimeOnce: each Action's Event runs through `invoke_event` as "Actions[k]".
            if let Some((c, _, v)) = script(prefab, o, "DoAtTimeOnce") {
                let mut actions = Vec::new();
                for (k, a) in array(v, "Actions")?.iter().enumerate() {
                    let calls = super::unity_event::calls(prefab, file, inst, field(a, "Event")?, "DoAtTimeOnce.Actions[].Event")?;
                    if !calls.is_empty() {
                        self.unity_events.insert((inst.objects[o].components[c], format!("Actions[{k}]")), calls);
                    }
                    let enabled = field(a, "Enabled")?.bool().ok_or("Enabled")?;
                    actions.push(super::destroyable::Action { Enabled: enabled, Delay: float(a, "Delay")?, Duration: float(a, "Duration")? });
                }
                self.do_at_time_onces.push(super::destroyable::DoAtTimeOnce { id: inst.objects[o].components[c], actions });
            }
        }
        let mut destroyables = HashMap::new();
        for o in 0..n {
            // VaseBehaviour extends DestroyableBehaviour (OnDebrisCreated gives the debris its sprite).
            let vase = script(prefab, o, "DestroyableBehaviour").is_none();
            let Some((c, _, v)) = script(prefab, o, "DestroyableBehaviour").or_else(|| script(prefab, o, "VaseBehaviour")) else { continue };
            let mut x = super::destroyable::Destroyable::default();
            if vase {
                x.vase = Some(self.sprite_at(&l, o));
            }
            load_values(x.fields(), v)?;
            x.id = inst.objects[o].components[c];
            x.PhysicalBehaviour = *l.arenas.physical.get(&o).unwrap_or_else(|| unimplemented!("mod-only: DestroyableBehaviour without a PhysicalBehaviour"));
            if l.arenas.limb.contains_key(&o) {
                unimplemented!("mod-only: DestroyableBehaviour on a limb: SendMessage order with LimbBehaviour");
            }
            x.after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
            x.index = c;
            let debris = field(v, "DebrisPrefab")?.pptr().ok_or("DebrisPrefab")?;
            x.DebrisPrefab = if debris.is_null() { None } else { Some(db.resolve(file, debris).ok_or("DebrisPrefab outside the data")?) };
            let ancestor = field(v, "VelocityAncestor")?.pptr().ok_or("VelocityAncestor")?;
            if !ancestor.is_null() {
                let (ao, _) = *prefab.index.get(&ObjRef { file, path_id: ancestor.path_id }).filter(|_| ancestor.file == 0).ok_or("VelocityAncestor outside the prefab")?;
                x.VelocityAncestor = Some(*l.arenas.physical.get(&ao).unwrap_or_else(|| unimplemented!("mod-only: DestroyableBehaviour.VelocityAncestor without a PhysicalBehaviour")));
            }
            x.OnBreak = event_calls(prefab, file, field(v, "OnBreak")?, inst, "DestroyableBehaviour.OnBreak", false)?;
            // DestroyableBehaviour.Awake: phys = GetComponent<PhysicalBehaviour>() is the arena link (`x.after_physical`, destroyable_start's physical).
            destroyables.insert(o, self.destroyables.len());
            self.destroyables.push(x);
        }
        let (mut explosives, mut delete_afters) = (HashMap::new(), HashMap::new());
        for o in 0..n {
            if let Some((c, _, v)) = script(prefab, o, "ExplosiveBehaviour") {
                let mut x = super::explosion::Explosive::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.physical = l.arenas.physical.get(&o).copied();
                if l.arenas.limb.contains_key(&o) {
                    unimplemented!("mod-only: ExplosiveBehaviour on a limb");
                }
                if !field(v, "Killzone")?.pptr().ok_or("Killzone")?.is_null() {
                    unimplemented!("mod-only: ExplosiveBehaviour with a serialized Killzone");
                }
                // Awake: BlockingLayers = 10752 when it is 0 (Objects, Debris, CollidingDebris + Bounds layers).
                let layers = field(field(v, "BlockingLayers")?, "m_Bits")?.i64().ok_or("BlockingLayers.m_Bits")? as u32;
                x.BlockingLayers = if layers == 0 { 10752 } else { layers };
                x.after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
                x.before_destroyable = script(prefab, o, "DestroyableBehaviour").is_some_and(|(dc, _, _)| c < dc);
                x.index = c;
                x.Tag = string(v, "Tag")?;
                x.OnExplodeCalls = event_calls(prefab, file, field(v, "OnExplode")?, inst, "ExplosiveBehaviour.OnExplode", false)?;
                let vfx = field(v, "VFXOverride")?.pptr().ok_or("VFXOverride")?;
                x.VFXOverride = if vfx.is_null() { None } else { Some(db.resolve(file, vfx).ok_or("VFXOverride outside the data")?) };
                explosives.insert(o, self.explosives.len());
                self.explosives.push(x);
                self.explosive_awake(self.explosives.len() - 1);
            }
            if let Some((c, _, v)) = script(prefab, o, "AddForceBehaviour") {
                let physical = *l.arenas.physical.get(&o).unwrap_or_else(|| unimplemented!("mod-only: AddForceBehaviour on a GameObject without a PhysicalBehaviour"));
                self.add_forces.push(super::misc::AddForce {
                    id: inst.objects[o].components[c],
                    physical,
                    ForceMode: field(v, "ForceMode")?.i64().ok_or("ForceMode")? as i32,
                    LocalAxis: floats::<2>(field(v, "LocalAxis")?)?,
                    ScaleWithSize: field(v, "ScaleWithSize")?.bool().ok_or("ScaleWithSize")?,
                });
            }
            if let Some((c, enabled, v)) = script(prefab, o, "DeleteAfterTime") {
                let mut x = super::explosion::DeleteAfter::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                delete_afters.insert(o, self.delete_afters.len());
                self.delete_afters.push(x);
            }
        }
        let mut firearms = HashMap::new();
        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "FirearmBehaviour") else { continue };
            let mut fa = Self::firearm_from_fields(db, file, v)?;
            fa.physical = l.arenas.physical.get(&o).copied();
            if fa.physical.is_none() {
                // A bare Rigidbody2D on the object (its NodeBody); without one, Fire's recoil throws.
                fa.node_body = Some(*node_rigidbodies.get(&o).unwrap_or_else(|| unimplemented!("mod-only: FirearmBehaviour without a Rigidbody2D (NullReferenceException in Fire)")));
                fa.node = Some(l.arenas.node[&o]);
            }
            (fa.id, fa.enabled) = (inst.objects[o].components[c], enabled);
            fa.ShotVolume = float(v, "ShotVolume")?;
            firearms.insert(o, self.firearms.len());
            self.firearms.push(fa);
            self.firearm_muzzle_smoke(db, self.firearms.len() - 1)?;
        }
        let mut machine_guns = HashMap::new();
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "MachineGunBehaviour") else { continue };
            let mut x = super::machinegun::MachineGun::default();
            load_values(x.fields(), v)?;
            x.id = inst.objects[o].components[c];
            // MachineGunBehaviour.Awake: rb and phys = GetComponent (the rigidbody is the physical's).
            x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: MachineGunBehaviour without a PhysicalBehaviour (NullReferenceException)"));
            let cartridge = field(v, "Cartridge")?.pptr().ok_or("Cartridge")?;
            let Some(cartridge) = db.resolve(file, cartridge) else { unimplemented!("mod-only: MachineGunBehaviour with a null Cartridge (NullReferenceException)") };
            let (_, _, cv) = db.mono(cartridge)?.ok_or("Cartridge is not a MonoBehaviour")?;
            x.Cartridge = super::ballistics::Cartridge::read(&cv)?;
            x.ExplosionSettings = explosion_parameters(field(v, "ExplosionSettings")?)?;
            x.Sounds = array(v, "Sounds")?.len();
            for (name, slot) in [("ImpactEffect", &mut x.ImpactEffect), ("RicochetEffect", &mut x.RicochetEffect)] {
                let p = field(v, name)?.pptr().ok_or(name)?;
                if !p.is_null() {
                    let r = db.resolve(file, p).ok_or_else(|| format!("MachineGunBehaviour.{name} outside the data"))?;
                    if db.prefab(r)?.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                        unimplemented!("mod-only: MachineGunBehaviour: an {name} with physics (its pose)");
                    }
                    *slot = Some(r);
                }
            }
            // Start's Effect pose for a flipped root: a particle system without physics.
            let effect = field(v, "Effect")?.pptr().ok_or("Effect")?;
            if !effect.is_null() {
                let (eo, _) = *prefab.index.get(&ObjRef { file, path_id: effect.path_id }).filter(|_| effect.file == 0).ok_or("MachineGunBehaviour.Effect outside the prefab")?;
                if std::iter::once(eo).chain(inst.descendants(eo)).any(|d| prefab.objects[d].components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                    unimplemented!("mod-only: MachineGunBehaviour.Effect with physics (its pose in Start)");
                }
            }
            machine_guns.insert(o, self.machine_guns.len());
            self.machine_guns.push(x);
        }
        // MinigunBehaviour: fires and resets the FirearmBehaviour field (a null one is a NullReferenceException on Update).
        let mut miniguns = HashMap::new();
        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "MinigunBehaviour") else { continue };
            let mut x = super::minigun::Minigun::default();
            load_values(x.fields(), v)?;
            x.id = inst.objects[o].components[c];
            x.enabled = enabled;
            x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: MinigunBehaviour without a PhysicalBehaviour (NullReferenceException)"));
            let fr = field(v, "FirearmBehaviour")?.pptr().ok_or("MinigunBehaviour.FirearmBehaviour")?;
            if fr.is_null() || fr.file != 0 {
                unimplemented!("mod-only: MinigunBehaviour.FirearmBehaviour null or outside the prefab (NullReferenceException in Update)");
            }
            let (fo, _) = *prefab.index.get(&ObjRef { file, path_id: fr.path_id }).ok_or("MinigunBehaviour.FirearmBehaviour: dangling reference")?;
            x.firearm = *firearms.get(&fo).unwrap_or_else(|| unimplemented!("mod-only: MinigunBehaviour.FirearmBehaviour on an object without a FirearmBehaviour in this clone"));
            miniguns.insert(o, self.miniguns.len());
            self.miniguns.push(x);
        }
        // AcceleratorGunBehaviour: the charge is PhysicalBehaviour's; a release spawns the bolt (accelerator.rs).
        let mut accelerators = HashMap::new();
        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "AcceleratorGunBehaviour") else { continue };
            let mut x = super::accelerator::Accelerator::default();
            load_values(x.fields(), v)?;
            x.id = inst.objects[o].components[c];
            x.enabled = enabled;
            let ph = field(v, "PhysicalBehaviour")?.pptr().ok_or("AcceleratorGunBehaviour.PhysicalBehaviour")?;
            if ph.is_null() || ph.file != 0 {
                unimplemented!("mod-only: AcceleratorGunBehaviour.PhysicalBehaviour null or outside the prefab (NullReferenceException in Update)");
            }
            let (po, _) = *prefab.index.get(&ObjRef { file, path_id: ph.path_id }).ok_or("AcceleratorGunBehaviour.PhysicalBehaviour: dangling reference")?;
            x.physical = l.arenas.physical.get(&po).copied().unwrap_or_else(|| unimplemented!("mod-only: AcceleratorGunBehaviour.PhysicalBehaviour on an object without a physical behaviour in this clone"));
            x.SingleShotSound = array(v, "SingleShotSound")?.len();
            // The barrel is the Rigidbody2D's of this GameObject (BarrelPosition and the recoil use the physical's body).
            let rbp = field(v, "Rigidbody")?.pptr().ok_or("AcceleratorGunBehaviour.Rigidbody")?;
            if rbp.is_null() || rbp.file != 0 || prefab.index.get(&ObjRef { file, path_id: rbp.path_id }).map(|&(ro, _)| ro) != Some(o) {
                unimplemented!("mod-only: AcceleratorGunBehaviour.Rigidbody not on its own GameObject");
            }
            let proj = field(v, "ProjectilePrefab")?.pptr().ok_or("AcceleratorGunBehaviour.ProjectilePrefab")?;
            if !proj.is_null() {
                x.ProjectilePrefab = Some(db.resolve(file, proj).ok_or("AcceleratorGunBehaviour.ProjectilePrefab outside the data")?);
            }
            accelerators.insert(o, self.accelerators.len());
            self.accelerators.push(x);
        }
        // AcceleratorBoltBehaviour: a bare Transform the gun's shots instantiate (its prefab has no physics).
        let mut accelerator_bolts = HashMap::new();
        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "AcceleratorBoltBehaviour") else { continue };
            if prefab.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                unimplemented!("mod-only: AcceleratorBoltBehaviour on a prefab with physics (its pose)");
            }
            let mut x = super::accelerator::AcceleratorBolt::default();
            load_values(x.fields(), v)?;
            x.id = inst.objects[o].components[c];
            x.enabled = enabled;
            x.Layers = field(field(v, "Layers")?, "m_Bits")?.i64().ok_or("AcceleratorBoltBehaviour.Layers.m_Bits")? as u32;
            x.ImmobiltyFieldLayer = field(field(v, "ImmobiltyFieldLayer")?, "m_Bits")?.i64().ok_or("AcceleratorBoltBehaviour.ImmobiltyFieldLayer.m_Bits")? as u32;
            for (name, slot) in [("ImpactEffect", &mut x.ImpactEffect), ("LargeImpactEffect", &mut x.LargeImpactEffect)] {
                let p = field(v, name)?.pptr().ok_or(name)?;
                if !p.is_null() {
                    let r = db.resolve(file, p).ok_or_else(|| format!("AcceleratorBoltBehaviour.{name} outside the data"))?;
                    if db.prefab(r)?.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                        unimplemented!("mod-only: AcceleratorBoltBehaviour: an {name} with physics (its pose)");
                    }
                    *slot = Some(r);
                }
            }
            accelerator_bolts.insert(o, self.accelerator_bolts.len());
            self.accelerator_bolts.push(x);
        }
        // ArchelixCasterBehaviour: Use starts BurstShot, which requests pooled bolts (archelix.rs).
        let mut archelix_casters = HashMap::new();
        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "ArchelixCasterBehaviour") else { continue };
            let mut x = super::archelix::ArchelixCaster::default();
            load_values(x.fields(), v)?;
            x.id = inst.objects[o].components[c];
            x.enabled = enabled;
            let ph = field(v, "PhysicalBehaviour")?.pptr().ok_or("ArchelixCasterBehaviour.PhysicalBehaviour")?;
            if ph.is_null() || ph.file != 0 {
                unimplemented!("mod-only: ArchelixCasterBehaviour.PhysicalBehaviour null or outside the prefab (NullReferenceException)");
            }
            let (po, _) = *prefab.index.get(&ObjRef { file, path_id: ph.path_id }).ok_or("ArchelixCasterBehaviour.PhysicalBehaviour: dangling reference")?;
            x.physical = l.arenas.physical.get(&po).copied().unwrap_or_else(|| unimplemented!("mod-only: ArchelixCasterBehaviour.PhysicalBehaviour on an object without a physical behaviour in this clone"));
            x.ShootSounds = array(v, "ShootSounds")?.len();
            x.MuzzleFlash = !field(v, "MuzzleFlash")?.pptr().ok_or("ArchelixCasterBehaviour.MuzzleFlash")?.is_null();
            // BarrelPosition uses the Rigidbody2D of this GameObject (the physical's body).
            let rbp = field(v, "Rigidbody")?.pptr().ok_or("ArchelixCasterBehaviour.Rigidbody")?;
            if rbp.is_null() || rbp.file != 0 || prefab.index.get(&ObjRef { file, path_id: rbp.path_id }).map(|&(ro, _)| ro) != Some(o) {
                unimplemented!("mod-only: ArchelixCasterBehaviour.Rigidbody not on its own GameObject");
            }
            let proj = field(v, "projectilePrefab")?.pptr().ok_or("ArchelixCasterBehaviour.projectilePrefab")?;
            if !proj.is_null() {
                x.projectilePrefab = Some(db.resolve(file, proj).ok_or("ArchelixCasterBehaviour.projectilePrefab outside the data")?);
            }
            archelix_casters.insert(o, self.archelix_casters.len());
            self.archelix_casters.push(x);
        }
        // ArchelixCasterBoltBehaviour: a pooled bolt without physics; its trail is a TrailRenderer on a child at the bolt's origin.
        let mut archelix_bolts = HashMap::new();
        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "ArchelixCasterBoltBehaviour") else { continue };
            if prefab.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                unimplemented!("mod-only: ArchelixCasterBoltBehaviour on a prefab with physics (its pose)");
            }
            if !enabled || o != 0 {
                unimplemented!("mod-only: ArchelixCasterBoltBehaviour disabled in the prefab, or not on the root");
            }
            let mut x = super::archelix::ArchelixBolt::default();
            load_values(x.fields(), v)?;
            x.id = inst.objects[o].components[c];
            x.Layers = field(field(v, "Layers")?, "m_Bits")?.i64().ok_or("ArchelixCasterBoltBehaviour.Layers.m_Bits")? as u32;
            let imp = field(v, "ImpactPoolable")?.pptr().ok_or("ArchelixCasterBoltBehaviour.ImpactPoolable")?;
            if !imp.is_null() {
                x.ImpactPoolable = Some(db.resolve(file, imp).ok_or("ArchelixCasterBoltBehaviour.ImpactPoolable outside the data")?);
            }
            let tp = field(v, "TrailRendererToDetach")?.pptr().ok_or("ArchelixCasterBoltBehaviour.TrailRendererToDetach")?;
            if tp.is_null() || tp.file != 0 {
                unimplemented!("mod-only: ArchelixCasterBoltBehaviour.TrailRendererToDetach null or outside the prefab (NullReferenceException)");
            }
            let (to, tc) = *prefab.index.get(&ObjRef { file, path_id: tp.path_id }).ok_or("ArchelixCasterBoltBehaviour.TrailRendererToDetach: dangling reference")?;
            let (tref, tcomp) = &prefab.objects[to].components[tc];
            if !matches!(tcomp, Component::Other(96)) {
                unimplemented!("mod-only: ArchelixCasterBoltBehaviour.TrailRendererToDetach is not a TrailRenderer");
            }
            let head = crate::assets::components::TrailHead::read(db.data(*tref))?;
            if head.autodestruct || !head.emitting || !head.enabled {
                unimplemented!("mod-only: ArchelixCasterBoltBehaviour: a TrailRenderer that is autodestructing, not emitting or disabled");
            }
            // The trail's points are the bolt's position: a child with an identity Transform.
            let identity = matches!(&prefab.objects[to].components[0].1, Component::Transform(t) if t.local_position == [0.0; 3] && t.local_rotation == [0.0, 0.0, 0.0, 1.0] && t.local_scale == [1.0; 3]);
            if prefab.objects[to].parent != Some(o) || !identity {
                unimplemented!("mod-only: ArchelixCasterBoltBehaviour: a TrailRenderer that is not a child at the bolt's origin");
            }
            x.trail_time = head.time;
            x.trail_min_distance = head.min_vertex_distance;
            x.trail_width_multiplier = head.width_multiplier;
            // Rendering: the trail as a world-space line (archelix_trail.rs), hidden until it has points.
            let data = super::archelix_trail::trail_line_data(db.data(*tref))?;
            x.trail_line = Some(self.add_line(db, file, data, inst.objects[to].components[tc], BareAnchor::Root, Vec::new(), prefab.objects[to].layer as i32, false));
            archelix_bolts.insert(o, self.archelix_bolts.len());
            self.archelix_bolts.push(x);
        }
        // UniversalBellBehaviour: its Start, FixedUpdate and Update touch the physical's body (universal_bell.rs). Its
        // clapper and clip sprites and labels are presentation, but Start dereferences them (NullReferenceException).
        let mut universal_bells = HashMap::new();
        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "UniversalBellBehaviour") else { continue };
            let mut x = super::universal_bell::UniversalBell::default();
            load_values(x.fields(), v)?;
            x.id = inst.objects[o].components[c];
            x.enabled = enabled;
            x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: UniversalBellBehaviour without a PhysicalBehaviour (NullReferenceException)"));
            for name in ["clapperSpriteRenderer", "DetailViewLabel"] {
                if field(v, name)?.pptr().ok_or(name)?.is_null() {
                    unimplemented!("mod-only: UniversalBellBehaviour.{name} is null (NullReferenceException in Start)");
                }
            }
            x.DetailViewLabel = self.text_field(&l, v, "DetailViewLabel")?;
            // gapC r26: clapperTransform, the clapper SpriteRenderer's object (Update sets its local rotation).
            let cl = field(v, "clapperSpriteRenderer")?.pptr().ok_or("clapperSpriteRenderer")?;
            if let Some(&(co, _)) = l.prefab.index.get(&ObjRef { file: l.file, path_id: cl.path_id }).filter(|_| cl.file == 0) {
                x.clapper = Some(inst.objects[co].components[0]);
            }
            universal_bells.insert(o, self.universal_bells.len());
            self.universal_bells.push(x);
        }
        // CounterBehaviour (counter.rs): Start dereferences its NixieTubeBehaviour; SendSignal's BroadcastMessage reaches use
        // wires on its object only, so a UseWire or another physical below it is refused.
        let mut counters = HashMap::new();
        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "CounterBehaviour") else { continue };
            let mut x = super::counter::Counter::default();
            load_values(x.fields(), v)?;
            x.id = inst.objects[o].components[c];
            x.enabled = enabled;
            x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: CounterBehaviour without a PhysicalBehaviour (NullReferenceException)"));
            if field(v, "NixieTubeBehaviour")?.pptr().ok_or("NixieTubeBehaviour")?.is_null() {
                unimplemented!("mod-only: CounterBehaviour.NixieTubeBehaviour is null (NullReferenceException in Start)");
            }
            // NixieTubeBehaviour.DigitRenderers (a null entry is skipped by SetValue).
            let nixie = l.object(field(v, "NixieTubeBehaviour")?)?.ok_or("CounterBehaviour.NixieTubeBehaviour outside its prefab")?;
            let (nc, _, nv) = script(prefab, nixie, "NixieTubeBehaviour").ok_or("CounterBehaviour.NixieTubeBehaviour is not a NixieTubeBehaviour")?;
            x.nixie = inst.objects[nixie].components[nc];
            for r in field(nv, "DigitRenderers")?.array().ok_or("NixieTubeBehaviour.DigitRenderers")? {
                x.digits.push(match l.object(r)? {
                    None => None,
                    Some(t) => Some(self.sprite_at(&l, t).ok_or("NixieTubeBehaviour.DigitRenderers: no SpriteRenderer")?),
                });
            }
            if inst.descendants(o).into_iter().any(|d| l.arenas.physical.contains_key(&d)) || script(prefab, o, "UseWireBehaviour").is_some() || inst.descendants(o).into_iter().any(|d| script(prefab, d, "UseWireBehaviour").is_some()) {
                unimplemented!("mod-only: CounterBehaviour.IsolatedActivation received below the counter");
            }
            counters.insert(o, self.counters.len());
            self.counters.push(x);
        }
        // PointerBehaviour (counter.rs): the hinge, its physical and RotorWheel's Transform (GetMouseAngle reads its position).
        let mut pointers = HashMap::new();
        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "PointerBehaviour") else { continue };
            let mut x = super::counter::Pointer::default();
            load_values(x.fields(), v)?;
            x.id = inst.objects[o].components[c];
            x.enabled = enabled;
            x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("PointerBehaviour.PhysicalBehaviour is null")?;
            x.own = l.object_xf(o);
            let hj = field(v, "Hinge")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: PointerBehaviour.Hinge null or outside its prefab (NullReferenceException)"));
            let at = *prefab.index.get(&ObjRef { file, path_id: hj.path_id }).ok_or("PointerBehaviour.Hinge: dangling reference")?;
            x.hinge = *l.arenas.hinge.get(&at).ok_or("PointerBehaviour.Hinge: not a HingeJoint2D")?;
            x.VolumeMultiplier = float(v, "VolumeMultiplier")?;
            x.ActiveLight = self.sprite_field(&l, v, "ActiveLight")?;
            for name in ["ActiveLight", "AudioSource"] {
                if field(v, name)?.pptr().ok_or(name)?.is_null() {
                    unimplemented!("mod-only: PointerBehaviour.{name} is null (NullReferenceException)");
                }
            }
            let wheel = l.object(field(v, "RotorWheel")?)?.ok_or("PointerBehaviour.RotorWheel is null")?;
            let (mut chain, mut up) = (Vec::new(), Some(wheel));
            x.rotor_anchor = loop {
                let Some(t) = up else { break BareAnchor::Root };
                if let Some(&p) = l.arenas.physical.get(&t) {
                    break BareAnchor::Physical(p);
                }
                if let Some(&i) = l.arenas.node.get(&t) {
                    break BareAnchor::Node(i);
                }
                chain.push(inst.objects[t].trs);
                up = inst.objects[t].parent;
            };
            x.rotor_chain = chain;
            pointers.insert(o, self.pointers.len());
            self.pointers.push(x);
        }
        // ToggleableMirrorBehaviour: FixedUpdate writes its physical's laser flags (mirror.rs).
        let mut toggleable_mirrors = HashMap::new();
        for o in 0..n {
            if script(prefab, o, "ToggleableMirrorBehaviour").is_none() {
                continue;
            }
            let mut x = super::mirror::ToggleableMirror::default();
            match l.arenas.physical.get(&o) {
                Some(&p) => {
                    x.physical = p;
                    x.block = self.get_property_block(super::visual::SpriteAt::Physical(p));
                }
                None => x.physical_null = true,
            }
            toggleable_mirrors.insert(o, self.toggleable_mirrors.len());
            self.toggleable_mirrors.push(x);
        }
        // GlowingHotMetalBehaviour and BurnableSpriteBehaviour: Awake's physical and property block (sprite_blocks.rs).
        for o in 0..n {
            if let Some((c, enabled, v)) = script(prefab, o, "GlowingHotMetalBehaviour") {
                let mut x = super::sprite_blocks::GlowingHotMetal::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.arenas.physical.get(&o).copied().expect("GlowingHotMetalBehaviour: the editor enforces RequireComponent(PhysicalBehaviour)");
                self.glowing_metals.push(x);
                let g = self.glowing_metals.len() - 1;
                self.glowing_hot_metal_awake(g);
            }
            if let Some((c, enabled, _)) = script(prefab, o, "ResizableSliceBehaviour") {
                let mut x = super::sprite_blocks::ResizableSlice::default();
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: ResizableSliceBehaviour without a PhysicalBehaviour (NullReferenceException in OnWillRenderObject)"));
                self.resizable_slices.push(x);
                let r = self.resizable_slices.len() - 1;
                self.resizable_slice_awake(r);
            }
            if let Some((c, enabled, _)) = script(prefab, o, "BurnableSpriteBehaviour") {
                let mut x = super::sprite_blocks::BurnableSprite::default();
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.arenas.physical.get(&o).copied().expect("BurnableSpriteBehaviour: the editor enforces RequireComponent(PhysicalBehaviour)");
                self.burnable_sprites.push(x);
                let b = self.burnable_sprites.len() - 1;
                self.burnable_sprite_awake(b);
            }
        }
        // BreakJointOnBurnBehaviour / BreakJointOnShotBehaviour: their Joint, Awake's GetComponent<PhysicalBehaviour> and
        // breakForce read, OnEnable's GetComponent<Joint2D> for an unset one (breakjoint.rs).
        for o in 0..n {
            let file = prefab.objects[o].game_object.file;
            for (c, (_, comp)) in prefab.objects[o].components.iter().enumerate() {
                let Component::Script { class, fields: v, .. } = comp else { continue };
                if class != "BreakJointOnBurnBehaviour" && class != "BreakJointOnShotBehaviour" {
                    continue;
                }
                let p = field(v, "Joint")?.pptr().ok_or("Joint")?;
                let joint = if p.is_null() {
                    None
                } else {
                    assert_eq!(p.file, 0, "{class}: a joint outside its prefab");
                    let k = prefab.index[&ObjRef { file, path_id: p.path_id }];
                    let Component::Joint2D(j) = &prefab.objects[k.0].components[k.1].1 else { unimplemented!("mod-only: {class}.Joint is not a Joint2D") };
                    Some((inst.objects[k.0].components[k.1], j.break_force))
                };
                let id = inst.objects[o].components[c];
                if class == "BreakJointOnBurnBehaviour" {
                    let (Joint, initialThreshold) = joint.unwrap_or_else(|| unimplemented!("mod-only: BreakJointOnBurnBehaviour.Awake with a null Joint (NullReferenceException)"));
                    let phys = l.arenas.physical.get(&o).copied();
                    self.break_joint_on_burns.push(super::breakjoint::BreakJointOnBurn { id, Joint, phys, initialThreshold, randomOffset: 1.0 });
                } else {
                    let own = || prefab.objects[o].components.iter().position(|(_, x)| matches!(x, Component::Joint2D(_))).map(|k| inst.objects[o].components[k]);
                    let Joint = joint.map(|j| j.0).or_else(own);
                    let (Chance, ShotIntensityInfluence) = (float(v, "Chance")?, float(v, "ShotIntensityInfluence")?);
                    self.break_joint_on_shots.push(super::breakjoint::BreakJointOnShot { id, Joint, Chance, ShotIntensityInfluence });
                }
            }
        }
        // LiquidSpreadAoEBehaviour: its LiquidContainer and the PhysicalBehaviour whose position it spreads from.
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "LiquidSpreadAoEBehaviour") else { continue };
            let file = prefab.objects[o].game_object.file;
            let lc = field(v, "LiquidContainer")?.pptr().ok_or("LiquidContainer")?;
            let LiquidContainer = (!lc.is_null()).then(|| {
                assert_eq!(lc.file, 0, "LiquidSpreadAoEBehaviour.LiquidContainer outside its prefab");
                let k = prefab.index[&ObjRef { file, path_id: lc.path_id }];
                inst.objects[k.0].components[k.1]
            });
            let phys = *l.arenas.physical.get(&o).unwrap_or_else(|| unimplemented!("mod-only: LiquidSpreadAoEBehaviour on an object without a PhysicalBehaviour"));
            self.liquid_spreads.push(super::liquid_spread::LiquidSpreadAoE {
                id: inst.objects[o].components[c],
                LiquidContainer,
                AmountMultiplier: float(v, "AmountMultiplier")?,
                LimbsOnly: field(v, "LimbsOnly")?.bool().ok_or("LimbsOnly")?,
                SpreadOnStart: field(v, "SpreadOnStart")?.bool().ok_or("SpreadOnStart")?,
                Range: float(v, "Range")?,
                Mask: field(field(v, "Mask")?, "m_Bits")?.i64().ok_or("Mask.m_Bits")? as u32,
                phys,
            });
        }
        // FancyBloodSplatController in an instantiated prefab: its fields, decal, and initialParent's position and
        // its own rotation (neither moves before Start). Under a physical it moves with it when the initialParent is
        // that physical's object or one of its direct children (the bleeding-particle pose; update_splat_pose).
        let mut splat_by_object = HashMap::new();
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "FancyBloodSplatController") else { continue };
            let file = prefab.objects[o].game_object.file;
            let mut x = super::misc::FancyBloodSplat::default();
            load_values(x.fields(), v)?;
            let dp = field(v, "Decal")?.pptr().ok_or("Decal")?;
            let decal = db.resolve(file, dp).unwrap_or_else(|| unimplemented!("mod-only: FancyBloodSplatController.Decal null or outside the data"));
            let k = self.load_decal_descriptor(db, decal)?;
            x.Decal = self.decal_descriptors[k].name.clone();
            x.CollisionLayers = field(field(v, "CollisionLayers")?, "m_Bits")?.i64().ok_or("CollisionLayers.m_Bits")? as u32;
            let parent = inst.objects[o].parent.unwrap_or_else(|| unimplemented!("mod-only: FancyBloodSplatController without a parent (initialParent.position: NullReferenceException)"));
            // The nearest physical above the splat, and no transform node or body above it.
            let mut nearest = None;
            // A Transform node that is the initialParent (a bare Rigidbody2D's transform, driven by its body) is the
            // attachment: its chain positions the splat. Nodes further up are not modelled.
            let mut node_attach = None;
            let mut up = Some(o);
            while let Some(t) = up {
                // The splat's own Transform is not an ancestor. Nodes above the nearest physical are part of its chain.
                if t != o && nearest.is_none() && l.arenas.node.contains_key(&t) {
                    if t != parent || node_attach.is_some() {
                        unimplemented!("mod-only: FancyBloodSplatController under a Transform node or body other than its initialParent");
                    }
                    node_attach = Some(l.arenas.node[&t]);
                }
                if let Some(&p) = l.arenas.physical.get(&t) {
                    if t == o {
                        unimplemented!("mod-only: FancyBloodSplatController on a physical object");
                    }
                    nearest.get_or_insert((t, p));
                }
                up = inst.objects[t].parent;
            }
            // Attached when the initialParent is the physical's object (identity pose) or its direct child (its local
            // TRS). Deeper, the initialParent's pose is not modelled.
            x.attached = match nearest {
                None => None,
                Some((t, p)) if t == parent => Some((p, crate::unity::transform::Trs { t: [0.0; 4], q: [0.0, 0.0, 0.0, 1.0], s: [1.0, 1.0, 1.0, 0.0] }, inst.objects[o].trs.q)),
                Some((t, p)) if inst.objects[parent].parent == Some(t) => Some((p, inst.objects[parent].trs, inst.objects[o].trs.q)),
                Some(_) => unimplemented!("mod-only: FancyBloodSplatController more than one Transform below its physical (its initialParent's pose is not modelled)"),
            };
            if let Some(n) = node_attach {
                x.attached = None;
                x.attached_node = Some((n, inst.objects[o].trs.q));
            }
            x.parent_position = inst.position(parent);
            x.rotation = inst.rotation(o);
            x.started = false;
            x.id = inst.objects[o].components[c];
            splat_by_object.insert(o, self.splats.len());
            self.splats.push(x);
        }
        // BloodTankBehaviour: its PhysicalParticles splat calls its OnCollide listener (blood_tank.rs).
        let mut blood_tanks = HashMap::new();
        let mut liquid_outlets = HashMap::new();
        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "BloodTankBehaviour") else { continue };
            let file = prefab.objects[o].game_object.file;
            let mut x = super::blood_tank::BloodTank::default();
            load_values(x.fields(), v)?;
            x.id = inst.objects[o].components[c];
            x.enabled = enabled;
            x.physical = *l.arenas.physical.get(&o).unwrap_or_else(|| unimplemented!("mod-only: BloodTankBehaviour without a PhysicalBehaviour (NullReferenceException in Awake)"));
            // PhysicalParticles: the FancyBloodSplatController of this prefab (Awake's OnCollide subscription on it).
            let pp = field(v, "PhysicalParticles")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: BloodTankBehaviour.PhysicalParticles null or outside its prefab (NullReferenceException in Awake)"));
            let (po, _) = *prefab.index.get(&ObjRef { file, path_id: pp.path_id }).ok_or("BloodTankBehaviour.PhysicalParticles: dangling reference")?;
            x.splat = *splat_by_object.get(&po).unwrap_or_else(|| unimplemented!("mod-only: BloodTankBehaviour.PhysicalParticles is not a FancyBloodSplatController of this prefab"));
            // WaterPuffParticles: its Transform (the first component of its object) decides whether the puff is submerged.
            let wp = field(v, "WaterPuffParticles")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: BloodTankBehaviour.WaterPuffParticles null or outside its prefab (NullReferenceException in UpdateActivation)"));
            let (wo, _) = *prefab.index.get(&ObjRef { file, path_id: wp.path_id }).ok_or("BloodTankBehaviour.WaterPuffParticles: dangling reference")?;
            x.water_puff = inst.objects[wo].components[0];
            x.ModeDisplay = self.text_field(&l, v, "ModeDisplay")?;
            self.splats[x.splat].on_collide = Some(super::container::Owner::BloodTank(self.blood_tanks.len()));
            blood_tanks.insert(o, self.blood_tanks.len());
            self.blood_tanks.push(x);
            self.blood_tank_awake(self.blood_tanks.len() - 1);
        }
        // LiquidContainerHoleBehaviour: its PhysicalParticles splat calls its OnCollide listener (liquid_leak.rs).
        let mut liquid_holes = HashMap::new();
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "LiquidContainerHoleBehaviour") else { continue };
            let file = prefab.objects[o].game_object.file;
            let reference = |name: &str| -> Result<usize, String> {
                let r = field(v, name)?.pptr().filter(|p| !p.is_null() && p.file == 0).ok_or(format!("LiquidContainerHoleBehaviour.{name} is null"))?;
                Ok(prefab.index.get(&ObjRef { file, path_id: r.path_id }).ok_or(format!("LiquidContainerHoleBehaviour.{name}: dangling reference"))?.0)
            };
            let (po, wo) = (reference("PhysicalParticles")?, reference("WaterPuffParticles")?);
            let splat = *splat_by_object.get(&po).ok_or("LiquidContainerHoleBehaviour.PhysicalParticles is not a FancyBloodSplatController of this prefab")?;
            let number = |name: &str| field(v, name)?.f32().ok_or(format!("LiquidContainerHoleBehaviour.{name}"));
            let x = super::liquid_leak::LiquidHole {
                id: inst.objects[o].components[c],
                root: inst.objects[0].components[0],
                DrainRate: number("DrainRate")?,
                ForceMultiplier: number("ForceMultiplier")?,
                Source: None,
                splat,
                water_puff: inst.objects[wo].components[0],
            };
            self.splats[splat].on_hole = Some(self.liquid_holes.len());
            liquid_holes.insert(o, self.liquid_holes.len());
            self.liquid_holes.push(x);
        }
        // LiquidOutletBehaviour: its PhysicalParticles splat calls its OnCollide listener (liquid_outlet.rs).
        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "LiquidOutletBehaviour") else { continue };
            let file = prefab.objects[o].game_object.file;
            let mut x = super::liquid_outlet::LiquidOutlet::default();
            x.id = inst.objects[o].components[c];
            x.enabled = enabled;
            x.physical = l.in_arena(field(v, "Phys")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("LiquidOutletBehaviour.Phys is null")?;
            let pp = field(v, "PhysicalParticles")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: LiquidOutletBehaviour.PhysicalParticles null or outside its prefab (NullReferenceException in Awake)"));
            let (po, _) = *prefab.index.get(&ObjRef { file, path_id: pp.path_id }).ok_or("LiquidOutletBehaviour.PhysicalParticles: dangling reference")?;
            x.splat = *splat_by_object.get(&po).unwrap_or_else(|| unimplemented!("mod-only: LiquidOutletBehaviour.PhysicalParticles is not a FancyBloodSplatController of this prefab"));
            let wp = field(v, "WaterPuffParticles")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: LiquidOutletBehaviour.WaterPuffParticles null or outside its prefab"));
            let (wo, _) = *prefab.index.get(&ObjRef { file, path_id: wp.path_id }).ok_or("LiquidOutletBehaviour.WaterPuffParticles: dangling reference")?;
            x.water_puff = inst.objects[wo].components[0];
            self.splats[x.splat].on_collide = Some(super::container::Owner::LiquidOutlet(self.liquid_outlets.len()));
            liquid_outlets.insert(o, self.liquid_outlets.len());
            self.liquid_outlets.push(x);
        }
        // GorseDroolingBehaviour: its LimbBehaviour, Projectile and where its Transform hangs.
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "GorseDroolingBehaviour") else { continue };
            let file = prefab.objects[o].game_object.file;
            let lp = field(v, "LimbBehaviour")?.pptr().ok_or("LimbBehaviour")?;
            if lp.is_null() || lp.file != 0 {
                unimplemented!("mod-only: GorseDroolingBehaviour.LimbBehaviour null or outside its prefab");
            }
            let lo = prefab.index[&ObjRef { file, path_id: lp.path_id }].0;
            let limb = *l.arenas.limb.get(&lo).unwrap_or_else(|| unimplemented!("mod-only: GorseDroolingBehaviour.LimbBehaviour not a linked limb"));
            let (mut chain, mut up) = (Vec::new(), Some(o));
            let anchor = loop {
                let Some(t) = up else { break BareAnchor::Root };
                if let Some(&p) = l.arenas.physical.get(&t) {
                    break BareAnchor::Physical(p);
                }
                if let Some(&i) = l.arenas.node.get(&t) {
                    break BareAnchor::Node(i);
                }
                chain.push(inst.objects[t].trs);
                up = inst.objects[t].parent;
            };
            let proj = field(v, "Projectile")?.pptr().ok_or("Projectile")?;
            let Projectile = if proj.is_null() { None } else { Some(db.resolve(file, proj).ok_or("GorseDroolingBehaviour.Projectile outside the data")?) };
            self.gorse_droolings.push(super::gorse::GorseDrooling {
                id: inst.objects[o].components[c],
                limb,
                anchor,
                chain,
                CanSee: field(field(v, "CanSee")?, "m_Bits")?.i64().ok_or("CanSee.m_Bits")? as u32,
                Projectile,
                FireClips: array(v, "FireClip")?.len(),
                Range: float(v, "Range")?,
                Force: float(v, "Force")?,
                step: 0,
                isAlive: false,
                emission_set: false,
            });
            let g = self.gorse_droolings.len() - 1;
            let id = self.gorse_droolings[g].id;
            self.add_embedded_ps(&l, prefab, inst, field(v, "ParticleSystem")?, id, super::EmbeddedDrive::Gorse(g))?;
        }
        // GorseEyeBehaviour: its Limb, Container and ranges.
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "GorseEyeBehaviour") else { continue };
            let file = prefab.objects[o].game_object.file;
            let lp = field(v, "Limb")?.pptr().ok_or("Limb")?;
            let limb = if lp.is_null() || lp.file != 0 {
                None
            } else {
                let lo = prefab.index[&ObjRef { file, path_id: lp.path_id }].0;
                Some(*l.arenas.limb.get(&lo).unwrap_or_else(|| unimplemented!("mod-only: GorseEyeBehaviour.Limb not a linked limb")))
            };
            let (rect, large, small) = (field(v, "Container")?, field(v, "LargeRange")?, field(v, "SmallRange")?);
            self.gorse_eyes.push(super::gorse::GorseEye {
                id: inst.objects[o].components[c],
                limb,
                container_position: [float(rect, "x")?, float(rect, "y")?],
                container_size: [float(rect, "width")?, float(rect, "height")?],
                SmallRandomRadius: float(v, "SmallRandomRadius")?,
                LargeRange: [float(large, "x")?, float(large, "y")?],
                SmallRange: [float(small, "x")?, float(small, "y")?],
                largeInterval: 0.0,
                smallInterval: 0.0,
                largeTimer: 0.0,
                smallTimer: 0.0,
                isRendering: true,
            });
        }
        // GorseProjectileBehaviour: its own colliders, body and DisperseEffect.
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "GorseProjectileBehaviour") else { continue };
            let file = prefab.objects[o].game_object.file;
            let node_body = *node_rigidbodies.get(&o).unwrap_or_else(|| unimplemented!("mod-only: GorseProjectileBehaviour without a bare Rigidbody2D on its GameObject"));
            let effect = field(v, "DisperseEffect")?.pptr().ok_or("DisperseEffect")?;
            let DisperseEffect = if effect.is_null() { None } else { Some(db.resolve(file, effect).ok_or("GorseProjectileBehaviour.DisperseEffect outside the data")?) };
            let colliders = prefab.objects[o].components.iter().enumerate().filter(|(_, (_, comp))| matches!(comp, Component::Collider2D(_))).map(|(k, _)| inst.objects[o].components[k]).collect();
            self.gorse_projectiles.push(super::gorse::GorseProjectile {
                id: inst.objects[o].components[c],
                Damage: float(v, "Damage")?,
                DisperseEffect,
                AcidChance: float(v, "AcidChance")?,
                colliders,
                node_body,
            });
        }
        // ScherbewerferBehaviour, NeedlerBehaviour, NeedleBehaviour, MachineGunProjectileBehaviour, PlaneBehaviour, WingBehaviour and
        // GlassShardBehaviour (old_weapons.rs).
        for o in 0..n {
            let file = prefab.objects[o].game_object.file;
            if let Some((c, _, v)) = script(prefab, o, "ScherbewerferBehaviour") {
                let mut x = super::old_weapons::Scherbewerfer::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.PhysicalBehaviour = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("ScherbewerferBehaviour.PhysicalBehaviour is null (NullReferenceException in Launch)")?;
                x.Projectile = l.in_arena(field(v, "Projectile")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("ScherbewerferBehaviour.Projectile is null (NullReferenceException in Launch)")?;
                x.own = l.object_xf(o);
                self.scherbewerfers.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "NeedlerBehaviour") {
                let mut x = super::old_weapons::Needler::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.physical = l.arenas.physical.get(&o).copied().ok_or("NeedlerBehaviour without a PhysicalBehaviour on its object")?;
                x.FireSounds = array(v, "FireSounds")?.len();
                let needle = field(v, "needle")?.pptr().ok_or("NeedlerBehaviour.needle")?;
                x.needle = if needle.is_null() { None } else { Some(db.resolve(file, needle).ok_or("NeedlerBehaviour.needle outside the data")?) };
                self.needlers.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "NeedleBehaviour") {
                let mut x = super::old_weapons::Needle::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.colliders = prefab.objects[o].components.iter().enumerate().filter(|(_, (_, k))| matches!(k, Component::Collider2D(_))).map(|(k, _)| inst.objects[o].components[k]).collect();
                x.rigidbody = prefab.objects[o].components.iter().position(|(_, k)| matches!(k, Component::Rigidbody2D(_))).map_or(0, |k| inst.objects[o].components[k]);
                let needle = field(v, "staticNeedle")?.pptr().ok_or("NeedleBehaviour.staticNeedle")?;
                x.staticNeedle = if needle.is_null() { None } else { Some(db.resolve(file, needle).ok_or("NeedleBehaviour.staticNeedle outside the data")?) };
                self.needles.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "WhOpening") {
                let mut x = super::old_weapons::WhOpening::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.GrowingCurve = Some(curve(field(v, "GrowingCurve")?)?);
                let wh = field(v, "Wh")?.pptr().ok_or("WhOpening.Wh")?;
                x.Wh = if wh.is_null() { None } else { Some(db.resolve(file, wh).ok_or("WhOpening.Wh outside the data")?) };
                // Awake: duration = AudioSource.clip.length.
                let a = field(v, "AudioSource")?.pptr().ok_or("WhOpening.AudioSource")?;
                let a = db.resolve(file, a).unwrap_or_else(|| unimplemented!("mod-only: WhOpening.AudioSource null (NullReferenceException in Awake)"));
                let source = crate::assets::classes::AudioSource::read(db.data(a))?;
                let clip = db.resolve(a.file, source.clip).unwrap_or_else(|| unimplemented!("mod-only: WhOpening AudioSource without a clip (NullReferenceException in Awake)"));
                x.clip_length = crate::assets::classes::AudioClip::read(db.data(clip))?.length;
                x.duration = x.clip_length;
                self.wh_openings.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "wh") {
                let mut x = super::wh::Wh::default();
                x.id = inst.objects[o].components[c];
                let u = field(v, "Uncharted")?.pptr().ok_or("wh.Uncharted")?;
                x.Uncharted = if u.is_null() { None } else { Some(db.resolve(file, u).ok_or("wh.Uncharted outside the data")?) };
                // The renderer is the object's own ParticleSystemRenderer.
                if !prefab.objects[o].components.iter().any(|(_, k)| matches!(k, Component::Other(super::water::PARTICLE_SYSTEM))) {
                    unimplemented!("mod-only: wh without a ParticleSystem on its object (OnWillRenderObject needs the renderer)");
                }
                x.system = self.embedded_ps.len();
                self.add_embedded_ps_at(&l, prefab, inst, o, x.id, super::EmbeddedDrive::Wh);
                self.whs.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "CannotShare") {
                let id = inst.objects[o].components[c];
                let asset = field(v, "Substitute")?.pptr().ok_or("CannotShare.Substitute")?;
                let Some(r) = db.resolve(file, asset) else { unimplemented!("mod-only: CannotShare.Substitute null (NullReferenceException in OnAfterDeserialise)") };
                let (_, mb, av) = db.mono(r)?.ok_or("SpawnableAsset")?;
                let prefab_ref = db.resolve(r.file, av.get("Prefab").and_then(Value::pptr).ok_or("SpawnableAsset.Prefab")?);
                let Some(substitute) = prefab_ref else { unimplemented!("mod-only: CannotShare.Substitute with a null Prefab (Instantiate throws)") };
                self.cannot_shares.push(super::cannot_share::CannotShare { id, substitute, name: mb.name.clone() });
            }
            if let Some((c, _, v)) = script(prefab, o, "RaygunBehaviour") {
                let mut x = super::old_weapons::Raygun::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.physical = l.arenas.physical.get(&o).copied().ok_or("RaygunBehaviour without a PhysicalBehaviour on its object")?;
                x.layersTohit = field(field(v, "layersTohit")?, "m_Bits")?.i64().ok_or("RaygunBehaviour.layersTohit.m_Bits")? as u32;
                x.beam = l.add_line(self, db, field(v, "beam")?, "RaygunBehaviour")?;
                x.beamCurve = Some(curve(field(v, "beamCurve")?)?);
                self.raygun_beams.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "StaticNeedleBehaviour") {
                let mut x = super::old_weapons::StaticNeedle::default();
                x.id = inst.objects[o].components[c];
                x.root = inst.objects[0].components[0];
                let explosion = field(v, "explosion")?.pptr().ok_or("StaticNeedleBehaviour.explosion")?;
                x.explosion = if explosion.is_null() { None } else { Some(db.resolve(file, explosion).ok_or("StaticNeedleBehaviour.explosion outside the data")?) };
                // GetComponent<ExplosiveBehaviour>() is only read when it stuck in a limb.
                x.explosive = *explosives.get(&o).ok_or("StaticNeedleBehaviour without an ExplosiveBehaviour (NullReferenceException)")?;
                if !field(v, "limb")?.pptr().ok_or("StaticNeedleBehaviour.limb")?.is_null() {
                    unimplemented!("mod-only: StaticNeedleBehaviour.limb serialised in its prefab");
                }
                self.static_needles.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "MachineGunProjectileBehaviour") {
                let mut x = super::old_weapons::MachineGunProjectile::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                self.machine_gun_projectiles.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "ShockwaveBehaviour") {
                let mut x = super::old_weapons::Shockwave::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.GrowingCurve = Some(curve(field(v, "GrowingCurve")?)?);
                self.shockwaves.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "GlassShardBehaviour") {
                let mut x = super::old_weapons::GlassShard::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                let _ = enabled;
                x.Rigidbody = l.in_arena(field(v, "Rigidbody")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("GlassShardBehaviour.Rigidbody is null (NullReferenceException)")?;
                self.glass_shards.push(x);
            }
        }
        let mut plane_at = HashMap::new();
        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "PlaneBehaviour") else { continue };
            let mut x = super::old_weapons::Plane::default();
            load_values(x.fields(), v)?;
            x.id = inst.objects[o].components[c];
            x.enabled = enabled;
            x.rigidbody = l.in_arena(field(v, "rigidbody")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("PlaneBehaviour.rigidbody is null (NullReferenceException)")?;
            x.physical = l.arenas.physical.get(&o).copied();
            x.index = c;
            x.after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
            plane_at.insert(o, self.planes.len());
            self.planes.push(x);
        }
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "WingBehaviour") else { continue };
            let mut x = super::old_weapons::Wing::default();
            load_values(x.fields(), v)?;
            x.id = inst.objects[o].components[c];
            x.rigidbody = l.arenas.physical.get(&o).copied().ok_or("WingBehaviour without a Rigidbody2D (NullReferenceException in FixedUpdate)")?;
            let lift = l.object(field(v, "liftPosition")?)?.ok_or("WingBehaviour.liftPosition is null (NullReferenceException in FixedUpdate)")?;
            x.liftPosition = inst.objects[lift].components[0];
            let plane = l.object(field(v, "plane")?)?.ok_or("WingBehaviour.plane is null (NullReferenceException in FuelTankRupture)")?;
            x.plane = *plane_at.get(&plane).ok_or("WingBehaviour.plane is no PlaneBehaviour")?;
            x.physical = l.arenas.physical.get(&o).copied();
            x.index = c;
            x.after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
            self.wings.push(x);
        }
        // WeightlessHelper: Bodies (the Rigidbody2D component ids); Awake keeps their masses.
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "WeightlessHelper") else { continue };
            let file = prefab.objects[o].game_object.file;
            let mut bodies = Vec::new();
            for b in field(v, "Bodies")?.array().ok_or("WeightlessHelper.Bodies")? {
                let q = b.pptr().filter(|q| !q.is_null() && q.file == 0).unwrap_or_else(|| unimplemented!("mod-only: WeightlessHelper.Bodies: a null or outside element (NullReferenceException)"));
                let (ro, rc) = *prefab.index.get(&ObjRef { file, path_id: q.path_id }).ok_or("WeightlessHelper.Bodies: dangling reference")?;
                bodies.push(inst.objects[ro].components[rc]);
            }
            self.weightless_helpers.push(super::physical::WeightlessHelper { id: inst.objects[o].components[c], bodies, original_mass: Vec::new() });
        }
        // FireballBehaviour: its own colliders, body, DAT and ExplosionEffect.
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "FireballBehaviour") else { continue };
            let file = prefab.objects[o].game_object.file;
            let node_body = *node_rigidbodies.get(&o).unwrap_or_else(|| unimplemented!("mod-only: FireballBehaviour without a bare Rigidbody2D on its GameObject"));
            let effect = field(v, "ExplosionEffect")?.pptr().ok_or("ExplosionEffect")?;
            let ExplosionEffect = if effect.is_null() { None } else { Some(db.resolve(file, effect).ok_or("FireballBehaviour.ExplosionEffect outside the data")?) };
            let rb = field(v, "Rigidbody")?.pptr().filter(|q| !q.is_null() && q.file == 0).unwrap_or_else(|| unimplemented!("mod-only: FireballBehaviour.Rigidbody null or outside its prefab (NullReferenceException)"));
            if prefab.index.get(&ObjRef { file, path_id: rb.path_id }).map(|x| x.0) != Some(o) {
                unimplemented!("mod-only: FireballBehaviour.Rigidbody on another GameObject");
            }
            let dat = field(v, "DAT")?.pptr().filter(|q| !q.is_null() && q.file == 0).unwrap_or_else(|| unimplemented!("mod-only: FireballBehaviour.DAT null or outside its prefab (NullReferenceException)"));
            let (dato, datc) = *prefab.index.get(&ObjRef { file, path_id: dat.path_id }).ok_or("FireballBehaviour.DAT: dangling reference")?;
            let colliders = prefab.objects[o].components.iter().enumerate().filter(|(_, (_, comp))| matches!(comp, Component::Collider2D(_))).map(|(k, _)| inst.objects[o].components[k]).collect();
            self.fireballs.push(super::excalibur::Fireball { id: inst.objects[o].components[c], ExplosionEffect, Range: float(v, "Range")?, colliders, node_body, DAT: inst.objects[dato].components[datc], hitOnce: false });
        }
        // SoundOnShakeBehaviour: Awake's GetComponent<Rigidbody2D>, its AudioSource and the lengths of Clips (shake.rs).
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "SoundOnShakeBehaviour") else { continue };
            let file = prefab.objects[o].game_object.file;
            let rb = prefab.objects[o].components.iter().position(|(_, x)| matches!(x, Component::Rigidbody2D(_))).map(|k| inst.objects[o].components[k]);
            let a = field(v, "AudioSource")?.pptr().ok_or("AudioSource")?;
            if a.file != 0 && !a.is_null() {
                unimplemented!("mod-only: SoundOnShakeBehaviour.AudioSource outside its prefab");
            }
            let clips = audio_clips(db, &mut self.audio.clip_cache, file, v, "Clips")?;
            // A null AudioSource or a missing Rigidbody2D throws in FixedUpdate.
            if a.is_null() || rb.is_none() {
                self.sound_on_shakes.push(super::shake::SoundOnShake {
                    id: inst.objects[o].components[c],
                    audio_null: a.is_null(),
                    rb_null: rb.is_none(),
                    ..Default::default()
                });
                continue;
            }
            let rb = rb.unwrap();
            let source = ObjRef { file, path_id: a.path_id };
            // AudioSource.m_Enabled, after m_GameObject: a disabled one never plays.
            if db.data(source)[12] == 0 {
                unimplemented!("mod-only: SoundOnShakeBehaviour with a disabled AudioSource");
            }
            let k = prefab.index[&source];
            self.sound_on_shakes.push(super::shake::SoundOnShake {
                id: inst.objects[o].components[c],
                rb,
                AudioSource: inst.objects[k.0].components[k.1],
                Clips: clips,
                ShakeThreshold: float(v, "ShakeThreshold")?,
                previousVelocity: [0.0, 0.0],
                ..Default::default()
            });
        }
        // ParticleSystemSerialisationHelper: IsPlaying and its ParticleSystems (shake.rs).
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "ParticleSystemSerialisationHelper") else { continue };
            let id = inst.objects[o].components[c];
            let r = field(v, "ParticleSystems")?.pptr().ok_or("ParticleSystemSerialisationHelper.ParticleSystems")?;
            let null = l.object_of(r)?.is_none();
            let IsPlaying = field(v, "IsPlaying")?.bool().ok_or("ParticleSystemSerialisationHelper.IsPlaying")?;
            self.serialisation_helpers.push(super::shake::SerialisationHelper { id, IsPlaying, null });
            let h = self.serialisation_helpers.len() - 1;
            self.script_slots.insert(id, super::save_slots::Slot::SerialisationHelpers(h));
            self.add_embedded_ps(&l, prefab, inst, field(v, "ParticleSystems")?, id, super::EmbeddedDrive::SerialisationHelper(h))?;
        }
        // EmitParticlesOnShakeBehaviour: Awake's GetComponent<ParticleSystem> and GetComponent<Rigidbody2D> (shake.rs).
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "EmitParticlesOnShakeBehaviour") else { continue };
            let id = inst.objects[o].components[c];
            let rb = prefab.objects[o].components.iter().position(|(_, x)| matches!(x, Component::Rigidbody2D(_))).map(|k| inst.objects[o].components[k]);
            self.emit_on_shakes.push(super::shake::EmitOnShake { id, rb, ShakeThreshold: float(v, "ShakeThreshold")?, EmissionCount: float(v, "EmissionCount")?, previousVelocity: [0.0; 2] });
            if prefab.objects[o].components.iter().any(|(_, x)| matches!(x, Component::Other(super::water::PARTICLE_SYSTEM))) {
                self.add_embedded_ps_at(&l, prefab, inst, o, id, super::EmbeddedDrive::EmitOnShake);
            } else {
                unimplemented!("mod-only: EmitParticlesOnShakeBehaviour without a ParticleSystem (Emit throws)");
            }
        }
        // FoliageBehaviour: Awake's GetComponentInParent<PhysicalBehaviour>, and its own SpriteRenderer (foliage.rs).
        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "FoliageBehaviour") else { continue };
            // FoliageBehaviour.Awake: Renderer (else its own SpriteRenderer), the property block and phys = GetComponentInParent below
            if !enabled {
                unimplemented!("mod-only: a disabled FoliageBehaviour (OnWillRenderObject still reaches it)");
            }
            let mut x = super::foliage::Foliage::default();
            load_values(x.fields(), v)?;
            x.id = inst.objects[o].components[c];
            if x.SyncWithParentPhysicalBehaviour {
                let mut up = Some(o);
                while let Some(t) = up {
                    if let Some(&p) = l.arenas.physical.get(&t) {
                        x.phys = Some(p);
                        break;
                    }
                    up = inst.objects[t].parent;
                }
            }
            // if (!Renderer) Renderer = GetComponent<SpriteRenderer>()
            let file = prefab.objects[o].game_object.file;
            let set = field(v, "Renderer")?.pptr().filter(|p| !p.is_null());
            let (ro, k) = match set {
                Some(p) if p.file == 0 => {
                    let (ro, k) = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).ok_or("FoliageBehaviour.Renderer: dangling reference")?;
                    if !matches!(prefab.objects[ro].components[k].1, Component::SpriteRenderer(_)) {
                        return Err("FoliageBehaviour.Renderer is no SpriteRenderer".into());
                    }
                    (ro, k)
                }
                Some(_) => unimplemented!("mod-only: FoliageBehaviour.Renderer outside its prefab"),
                None => {
                    let k = prefab.objects[o].components.iter().position(|(_, x)| matches!(x, Component::SpriteRenderer(_)));
                    (o, k.unwrap_or_else(|| unimplemented!("mod-only: FoliageBehaviour without a SpriteRenderer (NullReferenceException in Awake)")))
                }
            };
            let rid = inst.objects[ro].components[k];
            x.renderer = self.renderers.iter().position(|r| r.id == rid).unwrap_or_else(|| unimplemented!("mod-only: FoliageBehaviour: a SpriteRenderer with a null sprite"));
            // materialProperty = new MaterialPropertyBlock(); Renderer.GetPropertyBlock(materialProperty)
            x.ShaderPropertyName = string(v, "ShaderPropertyName")?;
            x.materialProperty = self.get_property_block(super::visual::SpriteAt::Renderer(x.renderer));
            self.foliages.push(x);
        }
        // WrenchBehaviour: OnCollisionEnter2D reaches it through its physical (wrench.rs); no Awake, Update or Start.
        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "WrenchBehaviour") else { continue };
            let mut x = super::wrench::Wrench::default();
            load_values(x.fields(), v)?;
            x.id = inst.objects[o].components[c];
            x.enabled = enabled;
            x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: WrenchBehaviour without a PhysicalBehaviour (its collisions are not delivered)"));
            x.index = c;
            x.after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
            self.wrenches.push(x);
        }
        // NoCollideWithBodyPart: its collider's collisions with a body part's limb are ignored (no_collide.rs).
        // Every one on an object (a car's frame holds three).
        for (o, (c, enabled, v)) in (0..n).flat_map(|o| scripts(prefab, o, "NoCollideWithBodyPart").map(move |s| (o, s))) {
            let mut x = super::no_collide::NoCollideWithBodyPart::default();
            load_values(x.fields(), v)?;
            x.enabled = enabled;
            x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: NoCollideWithBodyPart without a PhysicalBehaviour (its collisions are not delivered)"));
            x.index = c;
            x.after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
            // A null Collider never equals the receiver's own collider, so its callback does nothing (Collider stays 0).
            if let Some(p) = field(v, "Collider")?.pptr().filter(|p| !p.is_null()) {
                let (co, cc) = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).filter(|_| p.file == 0).ok_or("NoCollideWithBodyPart.Collider outside the prefab")?;
                x.Collider = inst.objects[co].components[cc];
            }
            self.no_collide_body_parts.push(x);
        }
        let mut bells = HashMap::new();
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "BellBehaviour") else { continue };
            let mut x = super::bell::Bell::default();
            load_values(x.fields(), v)?;
            x.id = inst.objects[o].components[c];
            let r = field(v, "PhysicalBehaviour")?.pptr().ok_or("PhysicalBehaviour")?;
            let own = script(prefab, o, "PhysicalBehaviour").map(|(pc, _, _)| prefab.objects[o].components[pc].0.path_id);
            if r.file != 0 || own != Some(r.path_id) {
                unimplemented!("mod-only: BellBehaviour.PhysicalBehaviour not on its own GameObject");
            }
            x.physical = l.arenas.physical[&o];
            bells.insert(o, self.bells.len());
            self.bells.push(x);
        }
        let mut launchers = HashMap::new();
        for o in 0..n {
            let Some((c, enabled, v)) = script(prefab, o, "ProjectileLauncherBehaviour") else { continue };
            let mut x = super::launcher::Launcher::default();
            load_values(x.fields(), v)?;
            (x.id, x.enabled) = (inst.objects[o].components[c], enabled);
            x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: ProjectileLauncherBehaviour without a PhysicalBehaviour (NullReferenceException)"));
            let asset = field(v, "projectileAsset")?.pptr().ok_or("projectileAsset")?;
            x.projectile = if !asset.is_null() {
                let r = db.resolve(file, asset).ok_or("projectileAsset outside the data")?;
                let (_, mb, av) = db.mono(r)?.ok_or("SpawnableAsset")?;
                let prefab = db.resolve(r.file, av.get("Prefab").and_then(Value::pptr).ok_or("SpawnableAsset.Prefab")?);
                Some((prefab.unwrap_or_else(|| unimplemented!("mod-only: ProjectileLauncherBehaviour: a SpawnableAsset with a null Prefab")), Some(mb.name.clone())))
            } else {
                let p = field(v, "projectilePrefab")?.pptr().ok_or("projectilePrefab")?;
                if p.is_null() { None } else { Some((db.resolve(file, p).ok_or("projectilePrefab outside the data")?, None)) }
            };
            let flash = field(v, "muzzleFlashPrefab")?.pptr().ok_or("muzzleFlashPrefab")?;
            if !flash.is_null() {
                let r = db.resolve(file, flash).ok_or("muzzleFlashPrefab outside the data")?;
                let effect = db.prefab(r)?;
                let poolable = effect.objects[0].tag == super::water::POOLABLE_TAG;
                if !poolable && effect.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                    unimplemented!("mod-only: ProjectileLauncherBehaviour: a muzzleFlashPrefab with physics (its pose)");
                }
                x.muzzle_flash = Some((r, poolable));
            }
            x.launch_sound = !field(v, "launchSound")?.pptr().ok_or("launchSound")?.is_null();
            x.LaunchClips = array(v, "LaunchClips")?.len();
            launchers.insert(o, self.launchers.len());
            self.launchers.push(x);
        }
        // UseEventTrigger.Use invokes Event (nothing sets its Action).
        for o in 0..n {
            if let Some((c, enabled, v)) = script(prefab, o, "UseEventTrigger") {
                let calls = event_calls(prefab, file, field(v, "Event")?, inst, "UseEventTrigger.Event", true)?;
                let only_once = field(v, "OnlyOnce")?.bool().ok_or("OnlyOnce")?;
                self.use_events.insert(inst.objects[o].components[c], UseEvent { enabled, only_once, use_count: 0, calls });
            }
        }
        let visuals = self.link_visuals(&l, &anchor_of)?;
        let mut android_sparks = HashMap::new();
        let mut inverters = HashMap::new();
        let mut obstruction_emitters = HashMap::new();
        let (mut act_on_collides, mut point_to_velocities, mut cars, mut tyres, mut flasks, mut truck_doors) = (HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new());
        let (mut glowtubes, mut cathode_lights, mut leds, mut detectors, mut sticky_grenades) = (HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new());
        let mut liquid_containers = HashMap::new();
        let (mut syringes, mut syringe_explosions) = (HashMap::new(), HashMap::new());
        let mut valves = HashMap::new();
        let mut liquid_canisters = HashMap::new();
        let mut liquid_filters = HashMap::new();
        let mut pulse_drums = HashMap::new();
        let mut liquidentifiers = HashMap::new();
        let mut centrifuges = HashMap::new();
        let mut vibrations = HashMap::new();
        let mut liquid_pumps = HashMap::new();
        let mut liquid_duplicators = HashMap::new();
        let mut timed_gates = HashMap::new();
        let mut lagboxes = HashMap::new();
        let mut gates = HashMap::new();
        let mut activation_transformers = HashMap::new();
        let mut hover_thrusters = HashMap::new();
        let mut hovercars = HashMap::new();
        let mut winches = HashMap::new();
        let mut conveyors = HashMap::new();
        let (mut reactor_cores, mut reactor_conduits, mut reactor_conduit_sets, mut reactor_meltdowns, mut do_at_times) = (HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new());
        let (mut industrial_lifts, mut random_sounds, mut acid_pools, mut vacuums, mut point_ats) = (HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new());
        let mut copper_wires: HashMap<usize, usize> = HashMap::new();
        let mut play_random_clips: HashMap<usize, usize> = HashMap::new();
        let (mut loop_sounds, mut joint_motor_sounds): (HashMap<usize, usize>, HashMap<usize, usize>) = (HashMap::new(), HashMap::new());
        let mut jms_links: Vec<(usize, Option<usize>)> = Vec::new();
        let mut eps: HashMap<usize, usize> = HashMap::new();
        // The references between the reactor scripts, resolved once every one is in its arena.
        let mut reactor_links: Vec<(usize, Vec<usize>, Option<usize>)> = Vec::new();
        let mut reactor_a5_displays = HashMap::new();
        let mut control_replacers = HashMap::new();
        let mut a5_links: Vec<(usize, Option<usize>, [Option<usize>; 4])> = Vec::new();
        let mut conduit_links: Vec<(usize, Option<usize>)> = Vec::new();
        let mut jukeboxes = HashMap::new();
        let mut antenna_receivers = HashMap::new();
        let mut couplers = HashMap::new();
        let mut lightning_entities = HashMap::new();
        let mut pyro_entities = HashMap::new();
        let mut seismic_explosions = HashMap::new();
        let mut energy_bolts = HashMap::new();
        let mut transmitters = HashMap::new();
        let (mut defibrillators, mut cardiopulmonary_bypasses) = (HashMap::new(), HashMap::new());
        let mut heart_monitors = HashMap::new();
        let mut excaliburs = HashMap::new();
        let mut sharp_on_all_sides = HashMap::new();
        let mut point_liquids = HashMap::new();
        for o in 0..n {
            if let Some((c, enabled, v)) = script(prefab, o, "WaterSoundBehaviour").filter(|x| x.1) {
                let _ = enabled;
                let r = field(v, "AudioRect")?;
                let (center, ext) = (field(r, "m_Center")?, field(r, "m_Extent")?);
                let (anchor, chain) = anchor_of(o);
                self.audio_scripts.push(super::audio::AudioScript::WaterSound {
                    id: inst.objects[o].components[c],
                    anchor,
                    chain,
                    center: [float(center, "x")?, float(center, "y")?, float(center, "z")?],
                    extents: [float(ext, "x")?, float(ext, "y")?, float(ext, "z")?],
                    soft_range: float(v, "SoftRange")?,
                });
            }
            if let Some((c, _, v)) = script(prefab, o, "RectangleAudioSourceBehaviour").filter(|x| x.1) {
                let r = field(v, "Rectangle")?;
                let (anchor, chain) = anchor_of(o);
                self.audio_scripts.push(super::audio::AudioScript::Rectangle {
                    id: inst.objects[o].components[c],
                    anchor,
                    chain,
                    max_volume: float(v, "MaxVolume")?,
                    min_volume: float(v, "MinVolume")?,
                    rect: [float(r, "x")?, float(r, "y")?, float(r, "width")?, float(r, "height")?],
                    max_distance: float(v, "MaxDistance")?,
                    auto: field(v, "AutoPlayAndStopBasedOnDistance")?.bool().ok_or("AutoPlayAndStopBasedOnDistance")?,
                });
            }
            if let Some((c, _, v)) = script(prefab, o, "LiquidContainerController") {
                let mut x = super::flask::LiquidContainer::default();
                (x.id, x.FillPercentage) = (inst.objects[o].components[c], float(v, "FillPercentage")?);
                x.SloshingIntensity = float(v, "SloshingIntensity")?;
                x.ParticleRadius = float(v, "ParticleRadius")?;
                x.LowFidelityLiquid = field(v, "LowFidelityLiquid")?.pptr().and_then(|m| db.resolve(file, m));
                // GetComponent<Renderer>(): the object's SpriteRenderer.
                x.renderer = prefab.objects[o].components.iter().position(|(_, k)| matches!(k, Component::SpriteRenderer(_))).map(|k| {
                    let rid = inst.objects[o].components[k];
                    self.renderers.iter().position(|r| r.id == rid).unwrap_or_else(|| unimplemented!("mod-only: LiquidContainerController: a SpriteRenderer with a null sprite"))
                });
                let b = field(v, "Container")?;
                let (center, extent) = (field(b, "m_Center")?, field(b, "m_Extent")?);
                // Bounds.min and max: center -/+ extents.
                (x.min, x.max) = ([float(center, "x")? - float(extent, "x")?, float(center, "y")? - float(extent, "y")?], [float(center, "x")? + float(extent, "x")?, float(center, "y")? + float(extent, "y")?]);
                x.center = [float(center, "x")?, float(center, "y")?, float(center, "z")?];
                x.extents = [float(extent, "x")?, float(extent, "y")?, float(extent, "z")?];
                // base.transform: the Transforms down from the nearest physical or node.
                (x.anchor, x.chain) = anchor_of(o);
                // Rigidbody (a Rigidbody2D of this prefab; null stops SimulateLiquid).
                if let Some(p) = field(v, "Rigidbody")?.pptr().filter(|p| !p.is_null()) {
                    let (ro, rc) = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).filter(|_| p.file == 0).ok_or("LiquidContainerController.Rigidbody outside the prefab")?;
                    x.Rigidbody = Some(inst.objects[ro].components[rc]);
                }
                liquid_containers.insert(o, self.liquid_containers.len());
                self.liquid_containers.push(x);
            }
        }
        // BloodTankBehaviour.LiquidContainer (its Update sets the fill and colour; a null one throws there).
        for (&o, &t) in &blood_tanks {
            let (_, _, v) = script(prefab, o, "BloodTankBehaviour").unwrap();
            self.blood_tanks[t].LiquidContainer = l.in_arena(field(v, "LiquidContainer")?, &liquid_containers, "LiquidContainerController")?;
        }
        // The sci-fi weapons the ActOnCollide and ActOnShot Actions call.
        let (mut blasters, mut blasterbolts, mut beamformers, mut scifi40s) = (HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new());
        let mut beam_cannons = HashMap::new();
        let (mut beam_repeaters, mut android_lasers) = (HashMap::new(), HashMap::new());
        let mut ray_cannons = HashMap::new();
        let mut railguns = HashMap::new();
        let mut laser_receivers = HashMap::new();
        let mut key_triggers = HashMap::new();
        let mut liquid_level_gates = HashMap::new();
        let mut fireworks = HashMap::new();
        let mut effect_guns = HashMap::new();
        let (mut lightning_guns, mut javelins, mut javelin_missiles) = (HashMap::new(), HashMap::new(), HashMap::new());
        // The local TRS chain from object `at` up to (excluding) `top`.
        let chain_below = |mut at: usize, top: usize, what: &str| -> Result<Vec<crate::unity::transform::Trs>, String> {
            let mut chain = Vec::new();
            while at != top {
                chain.push(inst.objects[at].trs);
                at = inst.objects[at].parent.ok_or_else(|| format!("{what} is outside its object"))?;
            }
            Ok(chain)
        };
        // A prefab an effect field instantiates: its pose is the script's, so it must have no physics.
        let no_physics = |db: &mut AssetDb, r: ObjRef| -> Result<(), String> {
            if db.prefab(r)?.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                unimplemented!("mod-only: a spawned effect with physics (its pose)");
            }
            Ok(())
        };
        for o in 0..n {
            if let Some((c, enabled, v)) = script(prefab, o, "LightningGunBehaviour") {
                let mut x = super::lightning_gun::LightningGun::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.Phys = l.in_arena(field(v, "Phys")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("LightningGunBehaviour.Phys is null")?;
                if l.arenas.physical.get(&o) != Some(&x.Phys) {
                    unimplemented!("mod-only: LightningGunBehaviour on another GameObject than its PhysicalBehaviour");
                }
                x.Layers = field(field(v, "Layers")?, "m_Bits")?.i64().ok_or("Layers.m_Bits")? as u32;
                let particles = l.object(field(v, "ParticleSystem")?)?.ok_or("LightningGunBehaviour.ParticleSystem is null (NullReferenceException)")?;
                x.RayOrigin = chain_below(particles, o, "LightningGunBehaviour.ParticleSystem")?;
                if x.RayOrigin.is_empty() {
                    unimplemented!("mod-only: LightningGunBehaviour.ParticleSystem on the gun's own object");
                }
                // The bolt, the target marker and the particles are presentation: their subtrees must not hold physics.
                let target = l.object(field(v, "TargetPosition")?)?.ok_or("LightningGunBehaviour.TargetPosition is null (NullReferenceException)")?;
                x.TargetObject = target;
                x.TargetChain = chain_below(target, o, "LightningGunBehaviour.TargetPosition")?.into_iter().skip(1).collect();
                if target == o {
                    unimplemented!("mod-only: LightningGunBehaviour.TargetPosition on the gun's own object");
                }


                for root in [particles, target] {
                    if std::iter::once(root).chain(inst.descendants(root)).any(|d| prefab.objects[d].components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                        unimplemented!("mod-only: LightningGunBehaviour: a ParticleSystem or TargetPosition object with physics");
                    }
                }
                lightning_guns.insert(o, self.lightning_guns.len());
                self.lightning_guns.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "JavelinBehaviour") {
                let mut x = super::javelin::Javelin::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("JavelinBehaviour.PhysicalBehaviour is null (NullReferenceException)")?;
                if l.arenas.physical.get(&o) != Some(&x.physical) {
                    unimplemented!("mod-only: JavelinBehaviour on another GameObject than its PhysicalBehaviour");
                }
                x.launcher = l.in_arena(field(v, "ProjectileLauncher")?, &launchers, "ProjectileLauncherBehaviour")?.ok_or("JavelinBehaviour.ProjectileLauncher is null (NullReferenceException)")?;
                x.LayerMask = field(field(v, "LayerMask")?, "m_Bits")?.i64().ok_or("LayerMask.m_Bits")? as u32;
                let ray = l.object(field(v, "RayStart")?)?.ok_or("JavelinBehaviour.RayStart is null (NullReferenceException)")?;
                x.RayStart = chain_below(ray, o, "JavelinBehaviour.RayStart")?;
                if x.RayStart.len() != 1 {
                    unimplemented!("mod-only: JavelinBehaviour.RayStart that is not a direct child of the launcher");
                }
                x.line = l.add_line(self, db, field(v, "Laser")?, "JavelinBehaviour")?;
                x.mute = field(v, "MuteBehaviour")?.pptr().filter(|p| !p.is_null() && p.file == 0).and_then(|p| prefab.index.get(&ObjRef { file, path_id: p.path_id })).map(|&(to, tc)| inst.objects[to].components[tc]);
                javelins.insert(o, self.javelins.len());
                self.javelins.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "JavelinMissileBehaviour") {
                let mut x = super::javelin::JavelinMissile::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: JavelinMissileBehaviour without a PhysicalBehaviour (its collisions are not delivered)"));
                x.index = c;
                x.after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
                let trail = l.object(field(v, "TrailParticleEffect")?)?.unwrap_or_else(|| unimplemented!("mod-only: JavelinMissileBehaviour.TrailParticleEffect null (NullReferenceException)"));
                let glow = l.object(field(v, "Glow")?)?.unwrap_or_else(|| unimplemented!("mod-only: JavelinMissileBehaviour.Glow null (NullReferenceException)"));
                for root in [trail, glow] {
                    if std::iter::once(root).chain(inst.descendants(root)).any(|d| prefab.objects[d].components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)) || matches!(c, Component::Script { class, .. } if super::declares(class, "OnEnable") || super::declares(class, "OnDisable") || super::declares(class, "Update") || super::declares(class, "FixedUpdate")))) {
                        unimplemented!("mod-only: JavelinMissileBehaviour: a Trail or Glow object with physics or scripts");
                    }
                }
                x.trail = inst.objects[trail].components[0];
                x.glow = inst.objects[glow].components[0];
                for (name, slot) in [("ExplosionEffect", &mut x.ExplosionEffect), ("SparkPrefab", &mut x.SparkPrefab)] {
                    let p = field(v, name)?.pptr().ok_or(name)?;
                    if !p.is_null() {
                        let r = db.resolve(file, p).ok_or_else(|| format!("JavelinMissileBehaviour.{name} outside the data"))?;
                        no_physics(db, r)?;
                        *slot = Some(r);
                    }
                }
                javelin_missiles.insert(o, self.javelin_missiles.len());
                self.javelin_missiles.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "BlasterBehaviour") {
                let mut x = super::blaster::Blaster::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                // BlasterBehaviour.Awake: physicalBehaviour = GetComponent<PhysicalBehaviour>().
                // BlasterBehaviour.Start and BlasterBehaviour.OnFlip turn Muzzleflash (a ParticleSystem) on a mirrored blaster.
                x.FlipMuzzleFlash = field(v, "FlipMuzzleFlash")?.bool().ok_or("BlasterBehaviour.FlipMuzzleFlash")?;
                let mf = field(v, "Muzzleflash")?.pptr().ok_or("BlasterBehaviour.Muzzleflash")?;
                if !mf.is_null() {
                    let Some(&(mo, _)) = l.prefab.index.get(&ObjRef { file: l.file, path_id: mf.path_id }).filter(|_| mf.file == 0) else {
                        return Err("BlasterBehaviour.Muzzleflash: a reference outside the prefab".into());
                    };
                    x.muzzle = Some(inst.objects[mo].components[0]);
                }
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: BlasterBehaviour without a PhysicalBehaviour (NullReferenceException)"));
                let bolt = field(v, "Bolt")?.pptr().ok_or("BlasterBehaviour.Bolt")?;
                if !bolt.is_null() {
                    let r = db.resolve(file, bolt).ok_or("BlasterBehaviour.Bolt outside the data")?;
                    // The bolt's pose is the script's: Transform.right and position on a bare Transform.
                    if db.prefab(r)?.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                        unimplemented!("mod-only: BlasterBehaviour: a Bolt with physics (its pose)");
                    }
                    x.Bolt = Some(r);
                }
                x.Clips = array(v, "Clips")?.len();
                x.ScreenShakeMultiplier = float(v, "ScreenShakeMultiplier")?;
                blasters.insert(o, self.blasters.len());
                self.blasters.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "FireworksBehaviour") {
                let mut x = super::fireworks::Fireworks::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("FireworksBehaviour.PhysicalBehaviour is null")?;
                x.own = l.object_xf(o);
                for (name, list) in [("Bursts", &mut x.Bursts), ("LowChanceBursts", &mut x.LowChanceBursts)] {
                    for r in array(v, name)? {
                        let r = r.pptr().ok_or_else(|| format!("FireworksBehaviour.{name}"))?;
                        // A null entry stays the null ObjRef: Instantiate throws only when PickRandom picks it.
                        list.push(if r.is_null() { ObjRef { file: 0, path_id: 0 } } else { db.resolve(file, r).ok_or_else(|| format!("FireworksBehaviour.{name} outside the data"))? });
                    }
                }
                let nuke = field(v, "FireworksNuke")?.pptr().ok_or("FireworksBehaviour.FireworksNuke")?;
                if !nuke.is_null() {
                    x.FireworksNuke = Some(db.resolve(file, nuke).ok_or("FireworksBehaviour.FireworksNuke outside the data")?);
                }
                let kz = field(v, "Killzone")?.pptr().ok_or("FireworksBehaviour.Killzone")?;
                let (ko, _) = *prefab.index.get(&ObjRef { file, path_id: kz.path_id }).filter(|_| kz.file == 0 && !kz.is_null()).ok_or("FireworksBehaviour.Killzone outside the prefab")?;
                if prefab.objects[ko].components.iter().any(|(_, k)| matches!(k, Component::Rigidbody2D(_) | Component::Collider2D(_) | Component::Joint2D(_))) {
                    unimplemented!("mod-only: FireworksBehaviour.Killzone with physics (its scale)");
                }
                x.Killzone = inst.objects[ko].components[0];
                let id = x.id;
                self.fireworks.push(x);
                fireworks.insert(o, self.fireworks.len() - 1);
                self.add_embedded_ps(&l, prefab, inst, field(v, "ParticleSystem")?, id, super::EmbeddedDrive::Firework(self.fireworks.len() - 1))?;
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ParticleMachineBehaviour") {
                let id = inst.objects[o].components[c];
                let flag = |n: &str| v.get(n).and_then(Value::bool).ok_or(format!("ParticleMachineBehaviour.{n}"));
                let init = [flag("EmitSmoke")?, flag("EmitFire")?, flag("EmitElectricity")?];
                for (k, n) in ["SmokeParticles", "FireParticles", "ElectricityParticles"].into_iter().enumerate() {
                    self.add_embedded_ps(&l, prefab, inst, field(v, n)?, id, super::EmbeddedDrive::ParticleMachine { k, init, enabled })?;
                }
            }
            if let Some((c, _, v)) = script(prefab, o, "MagnetBehaviour") {
                self.add_embedded_ps(&l, prefab, inst, field(v, "particleSystem")?, inst.objects[o].components[c], super::EmbeddedDrive::Magnet)?;
            }
            if let Some((c, _, v)) = script(prefab, o, "IonThrusterBehaviour") {
                self.add_embedded_ps(&l, prefab, inst, field(v, "FX")?, inst.objects[o].components[c], super::EmbeddedDrive::IonThruster)?;
            }
            if let Some((c, enabled, v)) = script(prefab, o, "KeyTriggerBehaviour") {
                let mut x = super::laser::KeyTrigger::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.TextMesh = self.text_field(&l, v, "TextMesh")?;
                x.DescriptionGizmo = self.text_field(&l, v, "DescriptionGizmo")?;
                x.ActiveLight = self.sprite_field(&l, v, "ActiveLight")?;
                x.LightGradient = super::visual::Gradient::read(field(v, "LightGradient")?)?;
                x.enabled = enabled;
                x.KeyCode = field(v, "KeyCode")?.i64().ok_or("KeyTriggerBehaviour.KeyCode")? as i32;
                x.Description = string(v, "Description")?;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("KeyTriggerBehaviour.PhysicalBehaviour is null")?;
                x.own = l.object_xf(o);
                x.drawn = self.sprite_at(&l, o);
                // SendSignal's IsolatedActivation and the continuous one go to its own GameObject.
                if script(prefab, o, "UseWireBehaviour").is_some() {
                    unimplemented!("mod-only: KeyTriggerBehaviour.IsolatedActivation received by a UseWireBehaviour");
                }
                self.key_triggers.push(x);
                key_triggers.insert(o, self.key_triggers.len() - 1);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "LiquidLevelGateBehaviour") {
                let mut x = super::laser::LiquidLevelGate::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.Cursor = self.sprite_field(&l, v, "Cursor")?;
                x.CursorMaxDistance = float(v, "CursorMaxDistance")?;
                x.PassthroughLight = self.plain_field(&l, v, "PassthroughLight")?;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("LiquidLevelGateBehaviour.PhysicalBehaviour is null (Start: NullReferenceException)")?;
                self.liquid_level_gates.push(x);
                liquid_level_gates.insert(o, self.liquid_level_gates.len() - 1);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "LaserReceiverBehaviour") {
                let mut x = super::laser::LaserReceiver::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: LaserReceiverBehaviour without a PhysicalBehaviour (NullReferenceException in Start)"));
                x.Light = self.sprite_field(&l, v, "Light")?;
                // IsolatedActivation and IsolatedContinuousActivation go to its own GameObject.
                if script(prefab, o, "UseWireBehaviour").is_some() {
                    unimplemented!("mod-only: LaserReceiverBehaviour.IsolatedActivation received by a UseWireBehaviour");
                }
                self.laser_receivers.push(x);
                laser_receivers.insert(o, self.laser_receivers.len() - 1);
            }
            if let Some((c, _, v)) = script(prefab, o, "BeamCannonBehaviour") {
                let mut x = super::blaster::BeamCannon::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("BeamCannonBehaviour.PhysicalBehaviour is null")?;
                x.own = l.object_xf(o);
                x.mask = field(field(v, "LayerMask")?, "m_Bits")?.i64().ok_or("BeamCannonBehaviour.LayerMask.m_Bits")? as u32;
                x.id = inst.objects[o].components[c];
                x.line = l.add_line(self, db, field(v, "LineRenderer")?, "BeamCannonBehaviour")?;
                let glow = l.object(field(v, "GlowSprite")?)?.ok_or("BeamCannonBehaviour.GlowSprite is null")?;
                x.glow = Some(self.sprite_at(&l, glow).ok_or("BeamCannonBehaviour.GlowSprite: no SpriteRenderer")?);
                beam_cannons.insert(o, self.beam_cannons.len());
                self.beam_cannons.push(x);
                self.beam_cannon_awake(self.beam_cannons.len() - 1);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "BeamRepeaterBehaviour") {
                let mut x = super::blaster::BeamRepeater::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("BeamRepeaterBehaviour.PhysicalBehaviour is null")?;
                x.own = l.object_xf(o);
                x.mask = field(field(v, "LayerMask")?, "m_Bits")?.i64().ok_or("BeamRepeaterBehaviour.LayerMask.m_Bits")? as u32;
                x.ShootSounds = array(v, "ShootSounds")?.len();
                let r = field(v, "ImpactPoolable")?.pptr().ok_or("BeamRepeaterBehaviour.ImpactPoolable")?;
                if !r.is_null() {
                    x.ImpactPoolable = Some(db.resolve(file, r).ok_or("BeamRepeaterBehaviour.ImpactPoolable outside the data")?);
                }
                // Awake: dist.RateHz = ShootRate (IntervalSeconds = 1f / value).
                x.dist.set_RateHz(x.ShootRate as f32);
                if l.object(field(v, "LaserRenderer")?)?.is_some() {
                    x.line = Some(l.add_line(self, db, field(v, "LaserRenderer")?, "BeamRepeaterBehaviour")?);
                }
                beam_repeaters.insert(o, self.beam_repeaters.len());
                self.beam_repeaters.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "AndroidLaserBehaviour") {
                let mut x = super::blaster::AndroidLaser::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.trigger = l.in_arena(field(v, "trigger")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("AndroidLaserBehaviour.trigger is null")?;
                x.own = l.object_xf(o);
                x.HitLayer = field(field(v, "HitLayer")?, "m_Bits")?.i64().ok_or("AndroidLaserBehaviour.HitLayer.m_Bits")? as u32;
                android_lasers.insert(o, self.android_lasers.len());
                let id = x.id;
                self.android_lasers.push(x);
                self.add_embedded_ps(&l, prefab, inst, field(v, "particles")?, id, super::EmbeddedDrive::AndroidLaser(self.android_lasers.len() - 1))?;
            }
            if let Some((c, enabled, v)) = script(prefab, o, "RayCannonBehaviour") {
                let mut x = super::blaster::RayCannon::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("RayCannonBehaviour.PhysicalBehaviour is null")?;
                x.own = l.object_xf(o);
                x.mask = field(field(v, "LayerMask")?, "m_Bits")?.i64().ok_or("RayCannonBehaviour.LayerMask.m_Bits")? as u32;
                x.ExplosionParams = Some(explosion_parameters(field(v, "ExplosionParams")?)?);
                let r = field(v, "ExplosionPrefab")?.pptr().ok_or("RayCannonBehaviour.ExplosionPrefab")?;
                if !r.is_null() {
                    x.ExplosionPrefab = Some(db.resolve(file, r).ok_or("RayCannonBehaviour.ExplosionPrefab outside the data")?);
                }
                x.line = l.add_line(self, db, field(v, "RayRenderer")?, "RayCannonBehaviour")?;
                let glow = l.object(field(v, "GlowSprite")?)?.ok_or("RayCannonBehaviour.GlowSprite is null")?;
                x.glow = Some(self.sprite_at(&l, glow).ok_or("RayCannonBehaviour.GlowSprite: no SpriteRenderer")?);
                ray_cannons.insert(o, self.ray_cannons.len());
                self.ray_cannons.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "RailgunBehaviour") {
                let mut x = super::blaster::Railgun::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("RailgunBehaviour.PhysicalBehaviour is null")?;
                x.own = l.object_xf(o);
                x.mask = field(field(v, "LayerMask")?, "m_Bits")?.i64().ok_or("RailgunBehaviour.LayerMask.m_Bits")? as u32;
                x.Lights = array(v, "Lights")?.len();
                for light in array(v, "Lights")? {
                    let t = light.pptr().ok_or("Lights")?;
                    let (lo, lc) = *prefab.index.get(&ObjRef { file, path_id: t.path_id }).filter(|_| !t.is_null() && t.file == 0).unwrap_or_else(|| unimplemented!("mod-only: RailgunBehaviour.Lights: a null or outside entry"));
                    let lid = inst.objects[lo].components[lc];
                    // Awake: Duration = 0.3.
                    let lv = *self.visual_ids.get(&lid).unwrap_or_else(|| unimplemented!("mod-only: RailgunBehaviour.Lights: not a Translation, Rotation or ColorAnimationBehaviour"));
                    self.trivial_set_duration(lv, 0.3);
                    x.light_ids.push(lid);
                }
                x.ShootSounds = array(v, "ShootSounds")?.len();
                let r = field(v, "ImpactPoolable")?.pptr().ok_or("RailgunBehaviour.ImpactPoolable")?;
                if !r.is_null() {
                    x.ImpactPoolable = Some(db.resolve(file, r).ok_or("RailgunBehaviour.ImpactPoolable outside the data")?);
                }
                x.line = l.add_line(self, db, field(v, "TrailRenderer")?, "RailgunBehaviour")?;
                railguns.insert(o, self.railguns.len());
                self.railguns.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "BlasterboltBehaviour") {
                let mut x = super::blaster::Blasterbolt::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.layers = field(field(v, "layers")?, "m_Bits")?.i64().ok_or("BlasterboltBehaviour.layers.m_Bits")? as u32;
                for (name, slot) in [("ImpactEffect", &mut x.ImpactEffect), ("HitDecal", &mut x.HitDecal)] {
                    let p = field(v, name)?.pptr().ok_or(name)?;
                    if !p.is_null() {
                        let r = db.resolve(file, p).ok_or_else(|| format!("BlasterboltBehaviour.{name} outside the data"))?;
                        if db.prefab(r)?.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                            unimplemented!("mod-only: BlasterboltBehaviour: an {name} with physics (its pose)");
                        }
                        *slot = Some(r);
                    }
                }
                let decal = field(v, "ImpactDecal")?.pptr().ok_or("BlasterboltBehaviour.ImpactDecal")?;
                if !decal.is_null() {
                    let r = db.resolve(file, decal).ok_or("BlasterboltBehaviour.ImpactDecal outside the data")?;
                    x.ImpactDecal = Some(self.load_decal_descriptor(db, r)?);
                }
                if prefab.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                    unimplemented!("mod-only: BlasterboltBehaviour on a prefab with physics (its pose)");
                }
                // Trail: drawn by trail.rs (rendering only; its autodestruct is the script's trail_autodestruct).
                let tp = field(v, "Trail")?.pptr().ok_or("BlasterboltBehaviour.Trail")?;
                if !tp.is_null() && tp.file == 0 {
                    let (to, tc) = *prefab.index.get(&ObjRef { file, path_id: tp.path_id }).ok_or("BlasterboltBehaviour.Trail: dangling reference")?;
                    let (tref, tcomp) = &prefab.objects[to].components[tc];
                    if matches!(tcomp, Component::Other(96)) {
                        let data = crate::assets::components::TrailRendererData::read(db.data(*tref))?;
                        self.trail_add(db, file, data, inst.objects[to].components[tc], prefab.objects[to].layer as i32, inst.objects[to].active);
                    }
                }
                blasterbolts.insert(o, self.blasterbolts.len());
                self.blasterbolts.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "BeamformerBehaviour") {
                let mut x = super::scifi::Beamformer::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.phys = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: BeamformerBehaviour without a PhysicalBehaviour (NullReferenceException)"));
                x.LayersToHit = field(field(v, "LayersToHit")?, "m_Bits")?.i64().ok_or("BeamformerBehaviour.LayersToHit.m_Bits")? as i32;
                // Start: BeamLineRenderer.gameObject.SetActive(false) must only hide a line.
                let beam = l.object(field(v, "BeamLineRenderer")?)?.ok_or("BeamformerBehaviour.BeamLineRenderer is null")?;
                if std::iter::once(beam).chain(inst.descendants(beam)).any(|d| prefab.objects[d].components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)) || matches!(c, Component::Script { class, .. } if super::declares(class, "OnEnable") || super::declares(class, "OnDisable")))) {
                    unimplemented!("mod-only: BeamformerBehaviour.BeamLineRenderer on a GameObject with physics or an OnEnable/OnDisable script");
                }
                x.beam_go = inst.objects[beam].components[0];
                x.beam = l.add_line(self, db, field(v, "BeamLineRenderer")?, "BeamformerBehaviour")?;
                x.aim = l.add_line(self, db, field(v, "AimLineRenderer")?, "BeamformerBehaviour")?;
                beamformers.insert(o, self.beamformers.len());
                self.beamformers.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "GenericScifiWeapon40Behaviour") {
                let mut x = super::scifi::Scifi40::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: GenericScifiWeapon40Behaviour without a PhysicalBehaviour (NullReferenceException)"));
                x.LayersToHit = field(field(v, "LayersToHit")?, "m_Bits")?.i64().ok_or("GenericScifiWeapon40Behaviour.LayersToHit.m_Bits")? as u32;
                // Rigidbody is the physical's own Rigidbody2D.
                let rb = field(v, "Rigidbody")?.pptr().ok_or("GenericScifiWeapon40Behaviour.Rigidbody")?;
                let own = prefab.index.get(&ObjRef { file, path_id: rb.path_id }).filter(|_| rb.file == 0 && !rb.is_null()).is_some_and(|&(ro, rc)| ro == o && matches!(prefab.objects[ro].components[rc].1, Component::Rigidbody2D(_)));
                if !own {
                    unimplemented!("mod-only: GenericScifiWeapon40Behaviour.Rigidbody that is not the Rigidbody2D of its own GameObject");
                }
                let impact = field(v, "Impact")?.pptr().ok_or("GenericScifiWeapon40Behaviour.Impact")?;
                if !impact.is_null() {
                    let r = db.resolve(file, impact).ok_or("GenericScifiWeapon40Behaviour.Impact outside the data")?;
                    if db.prefab(r)?.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                        unimplemented!("mod-only: GenericScifiWeapon40Behaviour: an Impact with physics (its pose)");
                    }
                    x.Impact = Some(r);
                }
                scifi40s.insert(o, self.scifi40s.len());
                self.scifi40s.push(x);
            }
            for class in ["HealthGunBehaviour", "FreezeGunBehaviour"] {
                let Some((c, enabled, v)) = script(prefab, o, class) else { continue };
                let mut x = super::effect_gun::EffectGun::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.freeze = class == "FreezeGunBehaviour";
                // PhysicalBehaviour is [SkipSerialisation]: only read by the base class through transform here.
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: {class} without a PhysicalBehaviour on its GameObject"));
                let flash = field(v, "Muzzleflash")?.pptr().ok_or("EffectGunBehaviour.Muzzleflash")?;
                if !flash.is_null() {
                    let r = db.resolve(file, flash).ok_or("EffectGunBehaviour.Muzzleflash outside the data")?;
                    if db.prefab(r)?.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                        unimplemented!("mod-only: EffectGunBehaviour: a Muzzleflash with physics (its pose)");
                    }
                    x.Muzzleflash = Some(r);
                }
                x.ShotSounds = array(v, "ShotSounds")?.len();
                effect_guns.insert(o, self.effect_guns.len());
                self.effect_guns.push(x);
            }
        }
        // A UnityEvent of persistent calls: ExplosiveBehaviour.Activate (the mine), FirearmBehaviour.Shoot,
        // ProjectileLauncherBehaviour.Launch, DamagableMachineryBehaviour.ForceBreak, BlasterBehaviour.Shoot,
        // BeamformerBehaviour.Shoot, GenericScifiWeapon40Behaviour.Fire and PhysicalBehaviour.ForceSendUse, audio and particles.
        // `machinery` is `Game::machinery.len()` before this clone's DamagableMachineryBehaviours.
        let act_targets = |actions: &Value, machinery: usize, what: &str| -> Result<Vec<super::acts::ActTarget>, String> {
            use super::acts::ActTarget;
            let mut targets = Vec::new();
            for call in invoked_calls(actions) {
                if let Some(a) = audio_call(prefab, file, inst, call) {
                    targets.push(ActTarget::Audio(a));
                    continue;
                }
                let method = string(call, "m_MethodName")?;
                let class_of = |to: usize, tc: usize| match &prefab.objects[to].components[tc].1 {
                    Component::Script { class, .. } => class.as_str(),
                    _ => "",
                };
                match call_target(prefab, file, call) {
                    Some((to, tc)) if class_of(to, tc) == "ExplosiveBehaviour" && method == "Activate" => targets.push(ActTarget::Activate(explosives[&to])),
                    Some((to, tc)) if class_of(to, tc) == "FirearmBehaviour" && method == "Shoot" => targets.push(ActTarget::Shoot(firearms[&to])),
                    Some((to, tc)) if class_of(to, tc) == "ProjectileLauncherBehaviour" && method == "Launch" => targets.push(ActTarget::Launch(launchers[&to])),
                    Some((to, tc)) if class_of(to, tc) == "DamagableMachineryBehaviour" && method == "ForceBreak" => {
                        // The machinery loop below pushes this clone's DamagableMachineryBehaviours in object order.
                        let before = (0..to).filter(|&x| script(prefab, x, "DamagableMachineryBehaviour").is_some()).count();
                        targets.push(ActTarget::ForceBreak(machinery + before));
                    }
                    Some((to, tc)) if class_of(to, tc) == "DamagableMachineryBehaviour" && method == "OnEMPHit" => {
                        let before = (0..to).filter(|&x| script(prefab, x, "DamagableMachineryBehaviour").is_some()).count();
                        targets.push(ActTarget::MachineryEmp(machinery + before));
                    }
                    Some((to, tc)) if class_of(to, tc) == "ActOnShot" && method == "set_enabled" && call.get("m_Arguments").and_then(|a| a.get("m_BoolArgument")).is_some_and(|b| b.bool() == Some(false) || b.i64() == Some(0)) => {
                        targets.push(ActTarget::DisableActOnShot(inst.objects[to].components[tc]));
                    }
                    Some((to, tc)) if class_of(to, tc) == "JavelinMissileBehaviour" && method == "Explode" => targets.push(ActTarget::Explode(javelin_missiles[&to])),
                    Some((to, tc)) if class_of(to, tc) == "BlasterBehaviour" && method == "Shoot" => targets.push(ActTarget::Blast(blasters[&to])),
                    Some((to, tc)) if class_of(to, tc) == "BeamformerBehaviour" && method == "Shoot" => targets.push(ActTarget::Beam(beamformers[&to])),
                    Some((to, tc)) if class_of(to, tc) == "GenericScifiWeapon40Behaviour" && method == "Fire" => targets.push(ActTarget::Fire(scifi40s[&to])),
                    Some((to, tc)) if class_of(to, tc) == "PhysicalBehaviour" && method == "ForceSendUse" => targets.push(ActTarget::ForceSendUse(l.arenas.physical[&to])),
                    // Ignite(bool): the call's bool argument is ignoreFlammability (a UnityEvent's BoolArgument is 0 or 1).
                    Some((to, tc)) if class_of(to, tc) == "PhysicalBehaviour" && method == "Ignite" => {
                        let ignore = call.get("m_Arguments").and_then(|a| a.get("m_BoolArgument")).is_some_and(|b| b.bool() == Some(true) || b.i64() == Some(1));
                        targets.push(ActTarget::Ignite(l.arenas.physical[&to], ignore));
                    }
                    // The CarBehaviour calls car_calls keeps (the same argument reads).
                    Some((to, tc)) if class_of(to, tc) == "CarBehaviour" && method == "SetHealth" => {
                        let h = call.get("m_Arguments").and_then(|a| a.get("m_FloatArgument")).and_then(Value::f32).ok_or("CarBehaviour.SetHealth without its float argument")?;
                        targets.push(ActTarget::Car(inst.objects[to].components[tc], super::car::CarCall::SetHealth(h)));
                    }
                    Some((to, tc)) if class_of(to, tc) == "CarBehaviour" && method == "set_enabled" && call.get("m_Arguments").and_then(|a| a.get("m_BoolArgument")).is_some_and(|b| b.bool() == Some(false) || b.i64() == Some(0)) => {
                        targets.push(ActTarget::Car(inst.objects[to].components[tc], super::car::CarCall::Disable));
                    }
                    _ if presentation_call(prefab, file, call) => {}
                    _ => targets.extend(super::unity_event::call(prefab, file, inst, call, what)?.map(ActTarget::Audio)),
                }
            }
            Ok(targets)
        };
        let (mut buttons, mut displays, mut lasers) = (HashMap::new(), HashMap::new(), HashMap::new());
        let mut firearm_attachments = HashMap::new();
        let mut attachment_points: HashMap<usize, usize> = HashMap::new();
        let mut televisions = HashMap::new();
        for o in 0..n {
            if let Some((c, _, v)) = script(prefab, o, "ButtonBehaviour") {
                let mut x = super::button::Button::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.physical = l.arenas.physical.get(&o).copied();
                x.phys = l.in_arena(field(v, "phys")?, &l.arenas.physical, "PhysicalBehaviour")?;
                x.light = self.sprite_field(&l, v, "light")?;
                // FixedUpdate's press sends and the continuous listener's sends reach this object's receivers only.
                if prefab.objects[o].components.iter().any(|(_, comp)| matches!(comp, Component::Script { class, .. } if class == "UseWireBehaviour" || super::translated::receives(class, "IsolatedActivation") || super::translated::receives(class, "IsolatedContinuousActivation"))) {
                    unimplemented!("mod-only: ButtonBehaviour: IsolatedActivation or IsolatedContinuousActivation to a receiver on its own object");
                }
                let file = prefab.objects[o].game_object.file;
                let slider = field(v, "slider")?.pptr().filter(|p| !p.is_null()).unwrap_or_else(|| unimplemented!("mod-only: ButtonBehaviour.slider is null (NullReferenceException)"));
                assert_eq!(slider.file, 0, "ButtonBehaviour.slider outside its prefab");
                let (so, sc) = prefab.index[&ObjRef { file, path_id: slider.path_id }];
                if !matches!(&prefab.objects[so].components[sc].1, Component::Joint2D(j) if matches!(j.kind, JointKind::Slider { .. })) {
                    unimplemented!("mod-only: ButtonBehaviour.slider is not a SliderJoint2D");
                }
                (x.slider_object, x.slider_component) = (so, sc);
                buttons.insert(o, self.buttons.len());
                self.buttons.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "DisplayBehaviour") {
                let mut x = super::display::Display::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.Value = string(v, "Value")?;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.unwrap_or_else(|| unimplemented!("mod-only: DisplayBehaviour.PhysicalBehaviour is null (NullReferenceException in Start)"));
                x.TextMesh = self.text_field(&l, v, "TextMesh")?;
                x.Light = self.sprite_field(&l, v, "Light")?;
                // 1.28.41: ColouredRenderers (SpriteRenderers) and Jitter (a HologramJitter).
                x.ColouredRenderers = array(v, "ColouredRenderers")?
                    .iter()
                    .map(|r| {
                        Ok(match l.object(r)? {
                            None => None,
                            Some(t) => Some(self.sprite_at(&l, t).ok_or("DisplayBehaviour.ColouredRenderers: no SpriteRenderer")?),
                        })
                    })
                    .collect::<Result<_, String>>()?;
                let jitter = field(v, "Jitter")?.pptr().ok_or("DisplayBehaviour.Jitter: not a reference")?;
                if !jitter.is_null() {
                    let Some(&(jo, jc)) = l.prefab.index.get(&ObjRef { file: l.file, path_id: jitter.path_id }).filter(|_| jitter.file == 0) else {
                        return Err("DisplayBehaviour.Jitter: a reference outside the prefab".into());
                    };
                    x.Jitter = visuals.get(&(jo, jc)).copied().filter(|&j| matches!(self.visuals[j].kind, super::visual::Kind::Hologram { .. }));
                }
                x.own_sprite = self.sprite_at(&l, o);
                displays.insert(o, self.displays.len());
                self.displays.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "LaserBehaviour") {
                let mut x = super::laser::Laser::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("LaserBehaviour.PhysicalBehaviour is null")?;
                x.LayersToHit = field(field(v, "LayersToHit")?, "m_Bits")?.i64().ok_or("LaserBehaviour.LayersToHit.m_Bits")? as u32;
                let ep = field(v, "ep")?.pptr().ok_or("LaserBehaviour.ep")?;
                x.ep = if ep.is_null() { None } else { Some(db.resolve(file, ep).ok_or("LaserBehaviour.ep outside the data")?) };
                x.line = l.add_line(self, db, field(v, "LineRenderer")?, "LaserBehaviour")?;
                x.LaserColourGradient = Some(super::visual::Gradient::read(field(v, "LaserColourGradient")?)?);
                for (k, name) in ["ImpactParticles", "SmokeParticles"].into_iter().enumerate() {
                    let ps = field(v, name)?.pptr().ok_or("LaserBehaviour particles: not a reference")?;
                    if !ps.is_null() {
                        let Some(&(po, _)) = l.prefab.index.get(&ObjRef { file: l.file, path_id: ps.path_id }).filter(|_| ps.file == 0) else {
                            return Err("LaserBehaviour particles: a reference outside the prefab".into());
                        };
                        x.particles[k] = Some(inst.objects[po].components[0]);
                    }
                }
                lasers.insert(o, self.lasers.len());
                self.lasers.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "TelevisionBehaviour") {
                let mut x = super::television::Television::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = *l.arenas.physical.get(&o).ok_or("TelevisionBehaviour without a PhysicalBehaviour (GetComponent is null)")?;
                x.index = c;
                x.after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
                x.break_sounds = array(v, "BreakSounds")?.len();
                x.my_renderer = self.sprite_field(&l, v, "MyRenderer")?;
                x.screen_renderer = self.sprite_field(&l, v, "ScreenRenderer")?;
                x.glow = self.sprite_field(&l, v, "Glow")?;
                x.glow_colours = super::visual::Gradient::read(field(v, "GlowColours")?)?;
                let asset = |name: &str| -> Result<Option<ObjRef>, String> { Ok(db.resolve(file, field(v, name)?.pptr().ok_or(name.to_string())?)) };
                (x.broken_telly, x.alt_telly, x.alt_broken_telly) = (asset("BrokenTelly")?, asset("AltTelly")?, asset("AltBrokenTelly")?);
                // The VideoPlayer's own clip, then MessageVideo, B2Video and B3Video (VideoClip.length).
                let player = field(v, "Video")?.pptr().ok_or("Video")?;
                let (po, pk) = *prefab.index.get(&ObjRef { file, path_id: player.path_id }).ok_or("TelevisionBehaviour.Video outside the prefab")?;
                let player = db.data(prefab.objects[po].components[pk].0);
                // VideoPlayer: Behaviour (GameObject, enabled) then m_VideoClip.
                let own = crate::assets::classes::PPtr { file: i32::from_le_bytes(player[16..20].try_into().unwrap()), path_id: i64::from_le_bytes(player[20..28].try_into().unwrap()) };
                let named = |name: &str| field(v, name).and_then(|c| c.pptr().ok_or_else(|| name.to_string()));
                for clip in [Ok(own), named("MessageVideo"), named("B2Video"), named("B3Video")] {
                    let r = db.resolve(file, clip?).unwrap_or_else(|| unimplemented!("mod-only: TelevisionBehaviour: a null VideoClip (NullReferenceException)"));
                    x.clips.push(super::television::VideoRef { clip: r, length: crate::assets::components::VideoClip::read(db.data(r))?.length() });
                }
                x.B2Pos = floats(field(v, "B2Pos")?)?;
                x.b2_map = db.resolve(file, field(v, "B2Map")?.pptr().ok_or("B2Map")?);
                x.videoTime = match field(v, "videoTime")? {
                    Value::Double(d) => *d,
                    _ => return Err("TelevisionBehaviour.videoTime".into()),
                };
                televisions.insert(o, self.televisions.len());
                self.televisions.push(x);
            }
            for class in super::attachment::CLASSES {
                if let Some((c, enabled, v)) = script(prefab, o, class) {
                    let mut x = super::attachment::FirearmAttachment::default();
                    load_values(x.fields(), v)?;
                    x.id = inst.objects[o].components[c];
                    x.enabled = enabled;
                    x.physical = l.arenas.physical.get(&o).copied();
                    x.laser = lasers.get(&o).copied();
                    let int = |name: &str| field(v, name)?.i64().map(|x| x as i32).ok_or_else(|| format!("{name}: not an integer"));
                    (x.kind, ()) = match class {
                        "ScopeAttachmentBehaviour" => {
                            x.AccuracyPercent = int("AccuracyPercent")?;
                            (super::attachment::Kind::Scope, ())
                        }
                        "BatteryAttachmentBehaviour" => {
                            x.BaseCharge = float(v, "BaseCharge")?;
                            (super::attachment::Kind::Battery, ())
                        }
                        "ExplosiveAttachmentBehaviour" => {
                            x.ExplosionParams = explosion_parameters(field(v, "ExplosionParams")?)?;
                            (super::attachment::Kind::Explosive, ())
                        }
                        "FlashlightAttachmentBehaviour" => {
                            for i in 0..array(v, "Lights")?.len() {
                                let t = l.object(&array(v, "Lights")?[i])?.unwrap_or_else(|| unimplemented!("mod-only: FlashlightAttachmentBehaviour.Lights: a null entry (NullReferenceException)"));
                                x.lights.push(self.sprite_at(&l, t).ok_or("FlashlightAttachmentBehaviour.Lights: no SpriteRenderer")?);
                            }
                            (super::attachment::Kind::Flashlight, ())
                        }
                        _ => {
                            x.FireChance = int("FireChance")?;
                            (super::attachment::Kind::Incendiary, ())
                        }
                    };
                    // GetLocalJointAnchor reads the SpriteRenderer's sprite rect height.
                    let renderer = prefab.objects[o].components.iter().find_map(|(_, x)| match x {
                        Component::SpriteRenderer(r) => Some(r),
                        _ => None,
                    });
                    if let Some(r) = renderer {
                        let sprite = db.resolve(file, r.sprite).ok_or("a SpriteRenderer without a sprite")?;
                        x.sprite_height = Sprite::read(db.data(sprite))?.rect[3];
                    }
                    firearm_attachments.insert(o, self.firearm_attachments.len());
                    self.firearm_attachments.push(x);
                }
            }
            if let Some((c, _, v)) = script(prefab, o, "FirearmAttachmentPointBehaviour") {
                // Start: firearmBehaviour = transform.parent.GetComponent<FirearmBehaviour>().
                let firearm = prefab.objects[o].parent.and_then(|t| firearms.get(&t).copied());
                let colliders = prefab.objects[o].components.iter().enumerate().filter(|(_, (_, x))| matches!(x, Component::Collider2D(_))).map(|(k, _)| inst.objects[o].components[k]).collect();
                attachment_points.insert(o, self.attachment_points.len());
                self.attachment_points.push(super::attachment::AttachmentPoint { id: inst.objects[o].components[c], colliders, AttachmentType: field(v, "AttachmentType")?.i64().ok_or("AttachmentType")? as i32, firearm, ..Default::default() });
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ActOnShot") {
                let mut x = super::acts::ActOnShot::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.targets = act_targets(field(v, "Actions")?, self.machinery.len(), "ActOnShot")?;
                self.act_on_shots.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "ShotMessagePropagator") {
                // Targets: a Transform id for each GameObject (a null entry is skipped by Shot's (bool) check).
                let mut targets = Vec::new();
                for g in array(v, "Targets")? {
                    if let Some(x) = l.object(g)? {
                        targets.push(inst.objects[x].components[0]);
                    }
                }
                self.shot_propagators.push(super::ballistics::ShotMessagePropagator { id: inst.objects[o].components[c], targets });
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ActOnCollide") {
                let mut x = super::acts::ActOnCollide::default();
                load_values(x.fields(), v)?;
                (x.enabled, x.physical, x.index) = (enabled, l.arenas.physical.get(&o).copied(), c);
                x.id = inst.objects[o].components[c];
                x.after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
                if let Some(p) = field(v, "Collider")?.pptr().filter(|p| !p.is_null()) {
                    let (co, cc) = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).filter(|_| p.file == 0).ok_or("ActOnCollide.Collider outside the prefab")?;
                    x.Collider = Some(inst.objects[co].components[cc]);
                }
                x.targets = act_targets(field(v, "Actions")?, self.machinery.len(), "ActOnCollide")?;
                act_on_collides.insert(o, self.act_on_collides.len());
                self.act_on_collides.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "NoCollide") {
                let set = |name: &str| -> Result<Vec<Option<i32>>, String> {
                    array(v, name)?
                        .iter()
                        .map(|r| {
                            let p = r.pptr().ok_or("NoCollide: not a reference")?;
                            if p.is_null() {
                                return Ok(None);
                            }
                            let (co, cc) = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).filter(|_| p.file == 0).ok_or("NoCollide: a collider outside the prefab")?;
                            Ok(Some(inst.objects[co].components[cc]))
                        })
                        .collect()
                };
                let bis_a = field(v, "BisA")?.bool().ok_or("BisA")?;
                self.no_collides.push(super::acts::NoCollide { id: inst.objects[o].components[c], a: set("NoCollideSetA")?, b: set("NoCollideSetB")?, bis_a });
            }
            if let Some((c, _, v)) = script(prefab, o, "StickyGrenadeBehaviour") {
                let mut x = super::acts::StickyGrenade::default();
                x.id = inst.objects[o].components[c];
                x.light = self.sprite_field(&l, v, "light")?;
                x.ArmedLightColour = floats::<4>(field(v, "ArmedLightColour")?)?;
                x.HotLightColor = floats::<4>(field(v, "HotLightColor")?)?;
                x.physical = l.arenas.physical.get(&o).copied();
                x.index = c;
                x.after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
                let r = field(v, "Explosive")?.pptr().ok_or("Explosive")?;
                let (eo, _) = *prefab.index.get(&ObjRef { file, path_id: r.path_id }).filter(|_| r.file == 0).ok_or("StickyGrenadeBehaviour.Explosive outside the prefab")?;
                x.explosive = *explosives.get(&eo).ok_or("StickyGrenadeBehaviour.Explosive is not an ExplosiveBehaviour")?;
                // Awake: unstickyPropertySet from StickyPropertiesToIgnore.
                for a in array(v, "StickyPropertiesToIgnore")? {
                    let asset = a.pptr().ok_or("StickyPropertiesToIgnore")?;
                    let asset = db.resolve(file, asset).ok_or("StickyPropertiesToIgnore outside the data")?;
                    x.unsticky.push(self.physical_properties(db, asset)?);
                }
                sticky_grenades.insert(o, self.sticky_grenades.len());
                self.sticky_grenades.push(x);
            }
            // ActOnHingeJointLimit.FixedUpdate loops over Joints: with none, nothing happens.
            if let Some((_, _, v)) = script(prefab, o, "ActOnHingeJointLimit") {
                if !array(v, "Joints")?.is_empty() {
                    unimplemented!("mod-only: ActOnHingeJointLimit with Joints: HingeJoint2D.limitState");
                }
            }
            if let Some((_, _, sv)) = script(prefab, o, "AndroidSparkCreator") {
                let mut x = super::acts::AndroidSparkCreator::default();
                x.limb = *l.arenas.limb.get(&o).unwrap_or_else(|| unimplemented!("mod-only: AndroidSparkCreator without a LimbBehaviour (NullReferenceException in FixedUpdate)"));
                // spark: the GameObject Instantiate copies in FixedUpdate (a null one is an ArgumentException there).
                let spark = field(sv, "spark")?.pptr().ok_or("AndroidSparkCreator.spark")?;
                if !spark.is_null() {
                    x.spark = Some(db.resolve(file, spark).ok_or("AndroidSparkCreator.spark outside the data")?);
                }
                android_sparks.insert(o, self.android_sparks.len());
                self.android_sparks.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ObstructionEmitterBehaviour") {
                let mut x = super::obstruction_emitter::ObstructionEmitter::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = *l.arenas.physical.get(&o).unwrap_or_else(|| unimplemented!("mod-only: ObstructionEmitterBehaviour without a PhysicalBehaviour: transform of a node"));
                let p = field(v, "EdgeCollider")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: ObstructionEmitterBehaviour.EdgeCollider null or outside its prefab (NullReferenceException)"));
                let k = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).ok_or("ObstructionEmitterBehaviour.EdgeCollider: dangling reference")?;
                if !matches!(&prefab.objects[k.0].components[k.1].1, Component::Collider2D(col) if matches!(col.shape, crate::assets::components::ColliderShape::Edge { .. })) {
                    return Err("ObstructionEmitterBehaviour.EdgeCollider is not an EdgeCollider2D".into());
                }
                x.EdgeCollider = k;
                x.HitLayers = field(field(v, "HitLayers")?, "m_Bits")?.i64().ok_or("ObstructionEmitterBehaviour.HitLayers.m_Bits")? as u32;
                // UpdateActivation touches these unconditionally.
                x.line_null = l.object(field(v, "LineRenderer")?)?.is_none();
                x.audio_null = l.object(field(v, "AudioSource")?)?.is_none();
                if !x.line_null {
                    x.line = l.add_line(self, db, field(v, "LineRenderer")?, "ObstructionEmitterBehaviour")?;
                }
                obstruction_emitters.insert(o, self.obstruction_emitters.len());
                self.obstruction_emitters.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "InverterBehaviour") {
                let mut x = super::acts::Inverter::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                match l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")? {
                    Some(p) => x.physical = p,
                    None => x.physical_null = true,
                }
                inverters.insert(o, self.inverters.len());
                self.inverters.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "PointToVelocityBehaviour") {
                let mut x = super::acts::PointToVelocity::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.physical = *l.arenas.physical.get(&o).unwrap_or_else(|| unimplemented!("mod-only: PointToVelocityBehaviour without a PhysicalBehaviour: GetComponent<Rigidbody2D>() of a node"));
                point_to_velocities.insert(o, self.point_to_velocities.len());
                self.point_to_velocities.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "CarBehaviour") {
                let mut x = super::car::CarBehaviour::default();
                load_values(x.fields(), v)?;
                x.enabled = enabled;
                x.id = inst.objects[o].components[c];
                // Awake: Phys = GetComponent<PhysicalBehaviour>() unless serialized.
                let phys = l.in_arena(field(v, "Phys")?, &l.arenas.physical, "PhysicalBehaviour")?;
                x.own_phys = phys.is_none();
                x.Phys = phys.or_else(|| l.arenas.physical.get(&o).copied()).unwrap_or_else(|| unimplemented!("mod-only: CarBehaviour without a PhysicalBehaviour"));
                x.own_phys = x.own_phys || l.arenas.physical.get(&o) == Some(&x.Phys);
                x.AccelerationCurve = Some(curve(field(v, "AccelerationCurve")?)?);
                // Awake's AddComponent<AudioSource>() sits on audioSourcePosition, else the car's own object.
                let at = l.object(field(v, "audioSourcePosition")?)?.unwrap_or(o);
                let (anchor, chain) = l.anchor_chain(at);
                let ancestors = std::iter::successors(Some(at), |&x| inst.objects[x].parent).map(|x| inst.objects[x].components[0]).collect();
                x.engine_proto = super::audio::AudioSource { anchor, chain, ancestors, active: inst.objects[at].active, ..Default::default() };
                x.EngineLoudness = float(v, "EngineLoudness")?;
                cars.insert(o, self.cars.len());
                self.cars.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "TyreDeflationBehaviour") {
                let mut x = super::tyre::TyreDeflationBehaviour::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.Phys = l.in_arena(field(v, "Phys")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("TyreDeflationBehaviour.Phys is null")?;
                let p = field(v, "Collider")?.pptr().filter(|p| !p.is_null() && p.file == 0).ok_or("TyreDeflationBehaviour.Collider")?;
                x.collider = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).ok_or("TyreDeflationBehaviour.Collider outside the prefab")?;
                if !matches!(&prefab.objects[x.collider.0].components[x.collider.1].1, Component::Collider2D(c) if matches!(c.shape, crate::assets::components::ColliderShape::Circle { .. })) {
                    return Err("TyreDeflationBehaviour.Collider is not a CircleCollider2D".into());
                }
                let settings = field(v, "DeflatedColliderSettings")?;
                x.DeflatedRadius = float(settings, "Radius")?;
                x.DeflatedOffset.set(field(settings, "Offset")?)?;
                x.sounds = array(v, "TyreDeflationSound")?.len();
                let m = field(v, "SharedDeflatedMaterial")?.pptr().ok_or("TyreDeflationBehaviour.SharedDeflatedMaterial")?;
                x.SharedDeflatedMaterial = if m.is_null() { None } else { Some(db.resolve(file, m).ok_or("SharedDeflatedMaterial outside the data")?) };
                tyres.insert(o, self.tyres.len());
                self.tyres.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "GasTankBehaviour").or_else(|| script(prefab, o, "FlaskBehaviour")) {
                let mut x = super::flask::Flask::default();
                load_values(x.fields(), v)?;
                // GasTankBehaviour.Limits is (0, 20); FlaskBehaviour's (0, Capacity).
                x.limit = if script(prefab, o, "GasTankBehaviour").is_some() { 20.0 } else { x.Capacity };
                x.id = inst.objects[o].components[c];
                x.LiquidContainer = l.in_arena(field(v, "LiquidContainer")?, &liquid_containers, "LiquidContainerController")?;
                let start = field(v, "StartLiquid")?;
                x.StartLiquid = string(start, "LiquidID")?;
                x.StartAmount = float(start, "Amount")?;
                x.Phys = l.arenas.physical.get(&o).copied();
                flasks.insert(o, self.flasks.len());
                self.flasks.push(x);
            }
            if let Some(&k) = reuse.syringe.get(&o) {
                syringes.insert(o, k);
            } else if let Some((class, (c, _, v))) = super::syringe::CLASSES.iter().find_map(|k| script(prefab, o, k).map(|x| (*k, x))) {
                let mut x = super::syringe::Syringe::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.Phys = *l.arenas.physical.get(&o).ok_or("SyringeBehaviour without a PhysicalBehaviour")?;
                x.liquid = super::syringe::liquid_id(class).map(super::circulation::liquid);
                x.pink = class == "PinkSyringe";
                // SendMessage("Lodged"/"Dislodged") from a stab (stab.rs) finds the receiver by this class.
                self.physicals[x.Phys].lodge_receiver = class.to_string();
                if x.pink {
                    super::Field::set(&mut x.Fingerprint, field(v, "Fingerprint")?)?;
                }
                syringes.insert(o, self.syringes.len());
                self.syringes.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ValveBehaviour").or_else(|| script(prefab, o, "LiquidPressureValveBehaviour")) {
                let mut x = super::valve::Valve::default();
                x.pressure_only = script(prefab, o, "LiquidPressureValveBehaviour").is_some();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: ValveBehaviour without a PhysicalBehaviour (NullReferenceException in TurnRoutine's PlayClipOnce)"));
                x.ValveTurnCurve = Some(curve(field(v, "ValveTurnCurve")?)?);
                let wheel = l.object(field(v, "ValveTransform")?)?.unwrap_or_else(|| unimplemented!("mod-only: ValveBehaviour.ValveTransform is null (NullReferenceException in Start)"));
                let physics = |x: usize| l.arenas.physical.contains_key(&x) || l.arenas.node.contains_key(&x) || prefab.objects[x].components.iter().any(|(_, k)| matches!(k, Component::Rigidbody2D(_) | Component::Collider2D(_) | Component::Joint2D(_)));
                if physics(wheel) || inst.descendants(wheel).into_iter().any(physics) {
                    unimplemented!("mod-only: ValveBehaviour.ValveTransform with physics");
                }
                x.wheel = inst.objects[wheel].trs;
                x.wheel_obj = wheel;
                x.wheel_refs = self.subtree_renderers(&l, wheel);
                x.display = self.sprite_field(&l, v, "DisplayRenderer")?;
                if let Some(d) = x.display {
                    x.displayProperties = self.get_property_block(d);
                }
                valves.insert(o, self.valves.len());
                self.valves.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "DangleBehaviour") {
                let [px, py] = floats::<2>(field(v, "Pivot")?)?;
                let mut x = super::dangle::Dangle::new(inst.objects[o].components[c], enabled, [px, py], float(v, "Length")?, float(v, "AngleOffset")?, float(v, "Dampening")?);
                (x.anchor, x.chain) = l.anchor_chain(o);
                if x.chain.is_empty() {
                    unimplemented!("mod-only: DangleBehaviour on a PhysicalBehaviour's or a node's object");
                }
                x.renderer = match self.sprite_at(&l, o) {
                    Some(super::visual::SpriteAt::Renderer(r)) => Some(r),
                    _ => None,
                };
                x.refs = self.subtree_renderers(&l, o);
                self.dangles.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "LiquidCanisterBehaviour") {
                let mut x = super::liquid_canister::LiquidCanister::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: LiquidCanisterBehaviour without a PhysicalBehaviour (NullReferenceException)"));
                x.BuoyancyResponseCurve = Some(curve(field(v, "BuoyancyResponseCurve")?)?);
                x.Text = self.text_field(&l, v, "Text")?;
                liquid_canisters.insert(o, self.liquid_canisters.len());
                self.liquid_canisters.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "LiquidFilterBehaviour") {
                let mut x = super::liquid_filter::LiquidFilter::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: LiquidFilterBehaviour without a PhysicalBehaviour (NullReferenceException in BloodContainer.transform)"));
                liquid_filters.insert(o, self.liquid_filters.len());
                self.liquid_filters.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "LiquidPumpBehaviour") {
                let mut x = super::liquid_pump::LiquidPump::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("LiquidPumpBehaviour.PhysicalBehaviour is null")?;
                if l.arenas.physical.get(&o) != Some(&x.physical) {
                    unimplemented!("mod-only: LiquidPumpBehaviour.PhysicalBehaviour on another object (Start's GetComponent<PhysicalBehaviour>)");
                }
                x.display = self.sprite_field(&l, v, "DisplayRenderer")?;
                let file = prefab.objects[o].game_object.file;
                let asset = |name: &str| -> Result<Option<ObjRef>, String> { Ok(db.resolve(file, field(v, name)?.pptr().ok_or(name.to_string())?)) };
                (x.Push, x.Pull, x.Idle) = (asset("Push")?, asset("Pull")?, asset("Idle")?);
                liquid_pumps.insert(o, self.liquid_pumps.len());
                self.liquid_pumps.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "LiquidContainerLeakingBehaviour") {
                let file = prefab.objects[o].game_object.file;
                let hole = field(v, "HolePrefab")?.pptr().filter(|p| !p.is_null()).ok_or("LiquidContainerLeakingBehaviour.HolePrefab is null")?;
                let canister = liquid_canisters.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: LiquidContainerLeakingBehaviour.Source other than its object's LiquidCanisterBehaviour"));
                let number = |name: &str| field(v, name)?.f32().ok_or(format!("LiquidContainerLeakingBehaviour.{name}"));
                let mut saved_holes = Vec::new();
                for h in array(v, "SerialisableHoles")? {
                    let two = |n: &str| -> Result<[f32; 2], String> { let x = field(h, n)?; Ok([field(x, "x")?.f32().ok_or(n.to_string())?, field(x, "y")?.f32().ok_or(n.to_string())?]) };
                    let (p, d) = (two("LocalPos")?, two("LocalDir")?);
                    saved_holes.push([p[0], p[1], d[0], d[1], field(h, "Intensity")?.f32().ok_or("Intensity")?]);
                }
                let own = match l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")? { Some(p) => Some(p), None => l.arenas.physical.get(&o).copied() };
                if own != Some(self.liquid_canisters[canister].physical) {
                    unimplemented!("mod-only: LiquidContainerLeakingBehaviour.PhysicalBehaviour other than its canister's (Start's button goes on it)");
                }
                self.liquid_leaks.push(super::liquid_leak::LiquidLeak {
                    id: inst.objects[o].components[c],
                    DrainIntensityMultiplier: number("DrainIntensityMultiplier")?,
                    VisualPressureMultiplier: number("VisualPressureMultiplier")?,
                    physical: self.liquid_canisters[canister].physical,
                    HolePrefab: db.resolve(file, hole).ok_or("HolePrefab outside the data")?,
                    Source: super::container::Owner::LiquidCanister(canister),
                    HasHoles: field(v, "HasHoles")?.bool().unwrap_or(false),
                    holes: Vec::new(),
                    hole_data: Vec::new(),
                    // The prefab's own SerialisableHoles array; Start (menu_start) makes the holes when HasHoles is set.
                    SerialisableHoles: Some(saved_holes),
                });
            }
            if let Some((c, enabled, v)) = script(prefab, o, "HeartMonitorBehaviour") {
                let mut x = super::heart_monitor::HeartMonitor::default();
                x.NoWireAttachedText = match field(v, "NoWireAttachedText")? {
                    crate::assets::mono::Value::Str(s) => s.clone(),
                    _ => return Err("HeartMonitorBehaviour.NoWireAttachedText".into()),
                };
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.HeartRateText = self.text_field(&l, v, "HeartRateText")?;
                x.line = l.add_line(self, db, field(v, "LineRenderer")?, "HeartMonitorBehaviour")?;
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: HeartMonitorBehaviour without a PhysicalBehaviour (a wire's GetComponent finds it through one)"));
                heart_monitors.insert(o, self.heart_monitors.len());
                self.heart_monitors.push(x);
                self.heart_monitor_awake(self.heart_monitors.len() - 1);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ExcaliburBehaviour") {
                let mut x = super::excalibur::Excalibur::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                // Lodged reaches it through its physical's receiver name (stab.rs send_lodged).
                let p = *l.arenas.physical.get(&o).unwrap_or_else(|| unimplemented!("mod-only: ExcaliburBehaviour without a PhysicalBehaviour (its Lodged receiver is the physical's)"));
                self.physicals[p].lodge_receiver = "ExcaliburBehaviour".to_string();
                let file = prefab.objects[o].game_object.file;
                let fb = field(v, "FireBallPrefab")?.pptr().ok_or("FireBallPrefab")?;
                x.FireBallPrefab = if fb.is_null() { None } else { Some(db.resolve(file, fb).ok_or("ExcaliburBehaviour.FireBallPrefab outside the data")?) };
                x.physical = p;
                x.ChargeAudioSource = audio_source_field(prefab, inst, file, v, "ExcaliburBehaviour", "ChargeAudioSource")?;
                x.SpellChargeCurve = Some(curve(field(v, "SpellChargeCurve")?)?);
                let sp = field(v, "SpawnPoint")?.pptr().filter(|q| !q.is_null() && q.file == 0).unwrap_or_else(|| unimplemented!("mod-only: ExcaliburBehaviour.SpawnPoint null or outside its prefab (NullReferenceException in CastSpell)"));
                let mut up = Some(prefab.index.get(&ObjRef { file, path_id: sp.path_id }).ok_or("ExcaliburBehaviour.SpawnPoint: dangling reference")?.0);
                while let Some(t) = up.filter(|t| *t != o) {
                    x.spawn_chain.push(inst.objects[t].trs);
                    up = inst.objects[t].parent;
                }
                if up.is_none() {
                    unimplemented!("mod-only: ExcaliburBehaviour.SpawnPoint outside the Excalibur own transform");
                }
                excaliburs.insert(o, self.excaliburs.len());
                self.excaliburs.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "SharpOnAllSidesBehaviour") {
                let file = prefab.objects[o].game_object.file;
                let mut x = super::sharp_on_all_sides::SharpOnAllSides::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = *l.arenas.physical.get(&o).unwrap_or_else(|| unimplemented!("mod-only: SharpOnAllSidesBehaviour without a PhysicalBehaviour (NullReferenceException in FixedUpdate)"));
                // SharpCollider's fixtures (the query shape of OverlapCollider).
                let sc = field(v, "SharpCollider")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: SharpOnAllSidesBehaviour.SharpCollider null or outside its prefab (NullReferenceException in FixedUpdate)"));
                let (so, scc) = *prefab.index.get(&ObjRef { file, path_id: sc.path_id }).ok_or("SharpOnAllSidesBehaviour.SharpCollider: dangling reference")?;
                x.SharpCollider = inst.fixtures.get(&(so, scc)).cloned().unwrap_or_default();
                x.sharp_collider_id = inst.objects[so].components[scc];
                x.LayerMask = field(field(v, "LayerMask")?, "m_Bits")?.i64().ok_or("LayerMask.m_Bits")? as u32;
                // OnAfterDeserialise reconnects SerialisableVictims; a field the prefab omits is an empty array.
                if v.get("SerialisableVictims").and_then(Value::array).is_some_and(|a| !a.is_empty()) {
                    unimplemented!("mod-only: SharpOnAllSidesBehaviour.SerialisableVictims: OnAfterDeserialise (GUID lookup of the victims)");
                }
                sharp_on_all_sides.insert(o, self.sharp_on_all_sides.len());
                self.sharp_on_all_sides.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "TimedGateBehaviour") {
                let mut x = super::timed_gate::TimedGate::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: TimedGateBehaviour without a PhysicalBehaviour (NullReferenceException)"));
                // Awake: MuteBehaviour = GetComponent<MuteBehaviour>()
                x.mute = script(prefab, o, "MuteBehaviour").map(|(mc, _, _)| inst.objects[o].components[mc]);
                // Knob is dereferenced by DoLag and Update (NullReferenceException when it is null); a null MuteBehaviour throws in `muted`.
                x.knob_null = field(v, "Knob")?.pptr().ok_or("Knob")?.is_null();
                let knob = field(v, "Knob")?.pptr().ok_or("Knob")?;
                if !knob.is_null() {
                    let Some(&(ko, _)) = l.prefab.index.get(&ObjRef { file: l.file, path_id: knob.path_id }).filter(|_| knob.file == 0) else {
                        return Err("TimedGateBehaviour.Knob: a reference outside the prefab".into());
                    };
                    x.knob = Some(inst.objects[ko].components[0]);
                }
                x.GreenLight = self.sprite_field(&l, v, "GreenLight")?;
                x.RedLight = self.sprite_field(&l, v, "RedLight")?;
                timed_gates.insert(o, self.timed_gates.len());
                self.timed_gates.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "HoverThrusterBehaviour") {
                let mut x = super::hover_thruster::HoverThruster::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("HoverThrusterBehaviour.PhysicalBehaviour is null")?;
                x.own = l.object_xf(o);
                x.mask = field(field(v, "Layers")?, "m_Bits")?.i64().ok_or("HoverThrusterBehaviour.Layers.m_Bits")? as u32;
                x.points = hover_points(prefab, inst, file, o, v, "HoverThrusterBehaviour")?;
                self.hover_thrusters.push(x);
                hover_thrusters.insert(o, self.hover_thrusters.len() - 1);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "HovercarBehaviour") {
                let mut x = super::hover_thruster::Hovercar::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("HovercarBehaviour.PhysicalBehaviour is null")?;
                x.own = l.object_xf(o);
                // Update, OnEnable and OnDisable dereference Fan; the two loops are audio source fields (a null one throws where it plays).
                x.fan_null = field(v, "Fan")?.pptr().ok_or("Fan")?.is_null();
                if !x.fan_null {
                    x.fan = audio_source_field(prefab, inst, file, v, "HovercarBehaviour", "Fan")?;
                }
                x.mask = field(field(v, "Layers")?, "m_Bits")?.i64().ok_or("HovercarBehaviour.Layers.m_Bits")? as u32;
                x.points = hover_points(prefab, inst, file, o, v, "HovercarBehaviour")?;
                self.hovercars.push(x);
                hovercars.insert(o, self.hovercars.len() - 1);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "GateBehaviour") {
                let mut x = super::timed_gate::Gate::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("GateBehaviour.PhysicalBehaviour is null")?;
                // Awake, OnEnable, OnDisable, Update and OnWillRenderObject dereference Cursor, PowerLevelBar and PassthroughLight (a null one throws there).
                x.Cursor = self.sprite_field(&l, v, "Cursor")?;
                x.PowerLevelBar = self.sprite_field(&l, v, "PowerLevelBar")?;
                x.PassthroughLight = self.sprite_field(&l, v, "PassthroughLight")?;
                x.CursorMaxDistance = float(v, "CursorMaxDistance")?;
                // BroadcastMessage("IsolatedActivation") reaches its object and the descendants.
                let below = std::iter::once(o).chain(inst.descendants(o));
                if below.into_iter().any(|x| script(prefab, x, "UseWireBehaviour").is_some()) {
                    unimplemented!("mod-only: GateBehaviour.IsolatedActivation received by a UseWireBehaviour");
                }
                self.gates.push(x);
                gates.insert(o, self.gates.len() - 1);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ActivationTransformerBehaviour") {
                let mut x = super::timed_gate::ActivationTransformer::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?
                    .unwrap_or_else(|| unimplemented!("mod-only: ActivationTransformerBehaviour.PhysicalBehaviour is null (NullReferenceException)"));
                x.own = l.object_xf(o);
                if script(prefab, o, "UseWireBehaviour").is_some() {
                    unimplemented!("mod-only: ActivationTransformerBehaviour.IsolatedActivation received by a prefab's UseWireBehaviour");
                }
                activation_transformers.insert(o, self.activation_transformers.len());
                self.activation_transformers.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "LagboxBehaviour") {
                let mut x = super::timed_gate::Lagbox::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: LagboxBehaviour without a PhysicalBehaviour (NullReferenceException)"));
                x.mute = script(prefab, o, "MuteBehaviour").map(|(mc, _, _)| inst.objects[o].components[mc]);
                // Knob is dereferenced by Update and DoLag (NullReferenceException when null); a null MuteBehaviour throws in `muted`.
                x.knob_null = field(v, "Knob")?.pptr().ok_or("Knob")?.is_null();
                let knob = field(v, "Knob")?.pptr().ok_or("Knob")?;
                if !knob.is_null() {
                    let Some(&(ko, _)) = l.prefab.index.get(&ObjRef { file: l.file, path_id: knob.path_id }).filter(|_| knob.file == 0) else {
                        return Err("LagboxBehaviour.Knob: a reference outside the prefab".into());
                    };
                    x.knob = Some(inst.objects[ko].components[0]);
                }
                lagboxes.insert(o, self.lagboxes.len());
                self.lagboxes.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "DefibrillatorBehaviour") {
                let mut x = super::resuscitation::Defibrillator::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                // PlayClipOnce on the PhysicalBehaviour, TextScreen (Start, OnEnable) and both pads are used without a null check.
                let physical = l.physical(field(v, "PhysicalBehaviour")?)?;
                if physical.is_none() || field(v, "TextScreen")?.pptr().ok_or("TextScreen")?.is_null() {
                    unimplemented!("mod-only: DefibrillatorBehaviour.PhysicalBehaviour or TextScreen is null (NullReferenceException)");
                }
                x.physical = physical.unwrap();
                x.TextScreen = self.text_field(&l, v, "TextScreen")?;
                x.Pad1 = l.physical(field(v, "Pad1")?)?.unwrap_or_else(|| unimplemented!("mod-only: DefibrillatorBehaviour.Pad1 is null (NullReferenceException)"));
                x.Pad2 = l.physical(field(v, "Pad2")?)?.unwrap_or_else(|| unimplemented!("mod-only: DefibrillatorBehaviour.Pad2 is null (NullReferenceException)"));
                x.PadLayer = field(field(v, "PadLayer")?, "m_Bits")?.i64().ok_or("PadLayer.m_Bits")? as u32;
                let clip = field(v, "AudioIndicator")?.pptr().ok_or("AudioIndicator")?;
                x.AudioIndicator = if clip.is_null() { None } else { Some(crate::assets::classes::AudioClip::read(db.data(db.resolve(file, clip).ok_or("AudioIndicator outside the data")?))?.length) };
                defibrillators.insert(o, self.defibrillators.len());
                self.defibrillators.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "CardiopulmonaryBypassMachineBehaviour") {
                let mut x = super::resuscitation::CardiopulmonaryBypass::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: CardiopulmonaryBypassMachineBehaviour without a PhysicalBehaviour (a wire's GetComponent finds it through one)"));
                // UpdateActivation (Start, OnEnable) uses Cosmetics and StatusText, and OnDisable the LoopSource, without a null check.
                let cosmetics = field(v, "Cosmetics")?.pptr().ok_or("Cosmetics")?;
                if cosmetics.is_null() || cosmetics.file != 0 || field(v, "StatusText")?.pptr().ok_or("StatusText")?.is_null() || field(v, "LoopSource")?.pptr().ok_or("LoopSource")?.is_null() {
                    unimplemented!("mod-only: CardiopulmonaryBypassMachineBehaviour.Cosmetics, StatusText or LoopSource is null (NullReferenceException)");
                }
                x.StatusText = self.text_field(&l, v, "StatusText")?;
                x.Cosmetics = prefab.index[&ObjRef { file, path_id: cosmetics.path_id }].0;
                cardiopulmonary_bypasses.insert(o, self.cardiopulmonary_bypasses.len());
                self.cardiopulmonary_bypasses.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "SyringeExplosionBehaviour") {
                let mut x = super::syringe::SyringeExplosion::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                syringe_explosions.insert(o, self.syringe_explosions.len());
                self.syringe_explosions.push(x);
            }
            if let Some((_, _, v)) = script(prefab, o, "PointLiquidTransferBehaviour") {
                let mut x = super::flask::PointLiquidTransfer::default();
                load_values(x.fields(), v)?;
                x.Phys = *l.arenas.physical.get(&o).ok_or("PointLiquidTransferBehaviour without a PhysicalBehaviour")?;
                x.Flask = *flasks.get(&o).ok_or("PointLiquidTransferBehaviour without a flask BloodContainer")?;
                x.Layers = field(field(v, "Layers")?, "m_Bits")?.i64().ok_or("Layers.m_Bits")? as u32;
                point_liquids.insert(o, self.point_liquids.len());
                self.point_liquids.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "GlowtubeBehaviour") {
                let mut x = super::light::Glowtube::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.Phys = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("GlowtubeBehaviour.PhysicalBehaviour is null")?;
                let gas = field(v, "Gas")?.pptr().ok_or("Gas")?;
                x.Gas = if gas.is_null() { None } else { Some(db.resolve(file, gas).ok_or("GlowtubeBehaviour.Gas outside the data")?) };
                x.LightSprite = self.sprite_field(&l, v, "LightSprite")?;
                x.ToToggle = self.sprite_field(&l, v, "ToToggle")?;
                x.PowerUpperbound = float(v, "PowerUpperbound")?;
                x.PowerGradient = super::visual::Gradient::read(field(v, "PowerGradient")?)?;
                glowtubes.insert(o, self.glowtubes.len());
                self.glowtubes.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "CathodeLightBehaviour") {
                let mut x = super::light::CathodeLight::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.Phys = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("CathodeLightBehaviour.PhysicalBehaviour is null")?;
                x.own = l.object_xf(o);
                let gas = field(v, "Gas")?.pptr().ok_or("Gas")?;
                x.Gas = if gas.is_null() { None } else { Some(db.resolve(file, gas).ok_or("CathodeLightBehaviour.Gas outside the data")?) };
                x.LightSprite = self.sprite_field(&l, v, "LightSprite")?;
                x.ToToggle = self.sprite_field(&l, v, "ToToggle")?;
                x.PowerUpperbound = float(v, "PowerUpperbound")?;
                x.PowerGradient = super::visual::Gradient::read(field(v, "PowerGradient")?)?;
                cathode_lights.insert(o, self.cathode_lights.len());
                self.cathode_lights.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "LEDBulbBehaviour") {
                let mut x = super::light::LedBulb::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.Phys = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("LEDBulbBehaviour.PhysicalBehaviour is null")?;
                x.LightSprite = self.sprite_field(&l, v, "LightSprite")?;
                x.GlowSprite = self.sprite_field(&l, v, "GlowSprite")?;
                x.BulbMask = self.sprite_field(&l, v, "BulbMask")?;
                leds.insert(o, self.leds.len());
                self.leds.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "DetectorBehaviour") {
                let mut x = super::detector::Detector::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.Phys = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("DetectorBehaviour.PhysicalBehaviour is null")?;
                x.LayersToDetect = field(field(v, "LayersToDetect")?, "m_Bits")?.i64().ok_or("LayersToDetect.m_Bits")? as u32;
                x.line = l.add_line(self, db, field(v, "Line")?, "DetectorBehaviour")?;
                x.light = self.sprite_field(&l, v, "EnabledLight")?;
                // SendMessage("IsolatedActivation") and ("IsolatedContinuousActivation") reach the detector's own GameObject.
                if script(prefab, o, "UseWireBehaviour").is_some() {
                    unimplemented!("mod-only: DetectorBehaviour.IsolatedActivation received by a UseWireBehaviour");
                }
                for f in array(v, "PropertyFilter")? {
                    let asset = db.resolve(file, f.pptr().ok_or("PropertyFilter")?).ok_or("PropertyFilter outside the data")?;
                    x.PropertyFilter.push(self.physical_properties(db, asset)?);
                }
                detectors.insert(o, self.detectors.len());
                self.detectors.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "TruckDoorBehaviour") {
                let mut x = super::car::TruckDoor::default();
                load_values(x.fields(), v)?;
                // TruckDoorBehaviour.Awake: joint = GetComponent<HingeJoint2D>().
                x.joint = (0..prefab.objects[o].components.len()).find_map(|k| l.arenas.hinge.get(&(o, k)).copied());
                x.root_physical = l.arenas.physical.get(&0).copied();
                truck_doors.insert(o, self.truck_doors.len());
                self.truck_doors.push((inst.objects[o].components[c], x));
            }
        }
        let mut machinery = HashMap::new();
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "DamagableMachineryBehaviour") else { continue };
            let mut m = super::machinery::Machinery::default();
            load_values(m.fields(), v)?;
            m.PhysicalBehaviour = *l.arenas.physical.get(&o).ok_or("DamagableMachineryBehaviour without a PhysicalBehaviour")?;
            // SparkPrefab, else Resources.Load("Prefabs/BrokenElectronicsSpark").
            let spark = field(v, "SparkPrefab")?.pptr().ok_or("SparkPrefab")?;
            if spark.is_null() {
                m.SparkPrefab = db.resource("Prefabs/BrokenElectronicsSpark", 1)?.is_some();
            } else {
                if spark.file == 0 {
                    unimplemented!("mod-only: DamagableMachineryBehaviour.SparkPrefab: a scene object inside the prefab");
                }
                m.SparkPrefab = true;
                m.SparkPrefabSource = Some(db.resolve(file, spark).ok_or("DamagableMachineryBehaviour.SparkPrefab outside the data")?);
            }
            let explosion_prefab = field(v, "ExplosionPrefab")?.pptr().ok_or("ExplosionPrefab")?;
            m.ExplosionPrefab = !explosion_prefab.is_null();
            if m.ExplosionPrefab {
                m.ExplosionPrefabSource = Some(db.resolve(file, explosion_prefab).ok_or("DamagableMachineryBehaviour.ExplosionPrefab outside the data")?);
            }
            m.Explosion = explosion_parameters(field(v, "Explosion")?)?;
            for b in array(v, "BehavioursToToggle")? {
                if let Some(t) = b.pptr().filter(|p| !p.is_null()) {
                    let (to, tc) = prefab.index[&ObjRef { file, path_id: t.path_id }];
                    let Component::Script { class, .. } = &prefab.objects[to].components[tc].1 else { unreachable!() };
                    m.BehavioursToToggle.push((class.clone(), inst.objects[to].components[tc]));
                }
            }
            for g in array(v, "GameObjectsToToggle")? {
                if let Some(x) = l.object(g)? {
                    m.GameObjectsToToggle.push(inst.objects[x].components[0]);
                }
            }
            m.id = inst.objects[o].components[c];
            machinery.insert(o, self.machinery.len());
            self.machinery.push(m);
        }
        let mut airfoils = HashMap::new();
        let mut stunners = HashMap::new();
        let mut soaps = HashMap::new();
        let mut power_hammers = HashMap::new();
        let mut balloons = HashMap::new();
        let (mut metronomes, mut thermometers, mut infrared_thermometers) = (HashMap::new(), HashMap::new(), HashMap::new());
        let mut determined_anomalies = HashMap::new();
        let mut random_sprite_children = HashMap::new();
        let (mut physics_guns, mut activator_electrodes) = (HashMap::new(), HashMap::new());
        let (mut activation_toggles, mut power_gauges) = (HashMap::new(), HashMap::new());
        let (mut torches, mut flares) = (HashMap::new(), HashMap::new());
        let mut map_lifts = HashMap::new();
        let (mut bulbs, mut thrusterbeds, mut pistons, mut flamethrowers, mut rockets) = (HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new());
        let (mut generators, mut floodlights, mut spike_grenades, mut heating_elements, mut ray_guns) = (HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new());
        for o in 0..n {
            if let Some((c, enabled, v)) = script(prefab, o, "GeneratorBehaviour") {
                let mut x = super::devices::Generator::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.Phys = l.in_arena(field(v, "physicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("GeneratorBehaviour.physicalBehaviour is null")?;
                x.block = self.get_property_block(super::visual::SpriteAt::Physical(x.Phys));
                generators.insert(o, self.generators.len());
                self.generators.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "SingleFloodlightBehaviour") {
                let mut x = super::devices::Floodlight::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.ToToggle = l.object(field(v, "ToToggle")?)?;
                x.SwitchSounds = array(v, "SwitchSounds")?.len();
                x.OnSwitchSound = array(v, "OnSwitchSound")?.len();
                x.PhysicalBehaviour = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("SingleFloodlightBehaviour.PhysicalBehaviour is null")?;
                x.Renderer = self.sprite_field(&l, v, "Renderer")?;
                for h in array(v, "Highlights")? {
                    let t = l.object(field(h, "SpriteRenderer")?)?.ok_or("SingleFloodlightBehaviour.Highlight.SpriteRenderer is null")?;
                    let SpriteRenderer = self.sprite_at(&l, t).ok_or("SingleFloodlightBehaviour.Highlight.SpriteRenderer: no SpriteRenderer")?;
                    x.Highlights.push(super::devices::Highlight { SpriteRenderer, Colour: floats::<4>(field(h, "Colour")?)?, Intensity: field(h, "Intensity")?.f32().ok_or("Highlight.Intensity")?, PropertyBlock: super::props::PropBlock(Default::default()) });
                }
                floodlights.insert(o, self.floodlights.len());
                self.floodlights.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "SpikeGrenadeBehaviour") {
                let mut x = super::devices::SpikeGrenade::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.Phys = *l.arenas.physical.get(&o).ok_or("SpikeGrenadeBehaviour without a PhysicalBehaviour")?;
                for (name, slot) in [("SpikePrefab", &mut x.SpikePrefab), ("ExplosionVFX", &mut x.ExplosionVFX)] {
                    let p = field(v, name)?.pptr().ok_or(name)?;
                    *slot = if p.is_null() { None } else { Some(db.resolve(file, p).ok_or_else(|| format!("SpikeGrenadeBehaviour.{name} outside the data"))?) };
                }
                x.CollisionLayers = field(field(v, "CollisionLayers")?, "m_Bits")?.i64().ok_or("CollisionLayers.m_Bits")? as u32;
                x.Light = self.sprite_field(&l, v, "Light")?;
                spike_grenades.insert(o, self.spike_grenades.len());
                self.spike_grenades.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "HeatingElementBehaviour") {
                let mut x = super::devices::HeatingElement::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.Phys = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("HeatingElementBehaviour.PhysicalBehaviour is null")?;
                let m = l.object(field(v, "DamagableMachineryBehaviour")?)?.ok_or("HeatingElementBehaviour.DamagableMachineryBehaviour is null")?;
                x.Machinery = *machinery.get(&m).ok_or("HeatingElementBehaviour: no DamagableMachineryBehaviour on its object")?;
                x.ActivationLight = self.sprite_field(&l, v, "ActivationLight")?;
                x.Glow = self.sprite_field(&l, v, "Glow")?;
                heating_elements.insert(o, self.heating_elements.len());
                self.heating_elements.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "TemperatureRayGunBehaviour") {
                let mut x = super::devices::RayGun::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.Phys = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("TemperatureRayGunBehaviour.PhysicalBehaviour is null")?;
                x.Layers = field(field(v, "Layers")?, "m_Bits")?.i64().ok_or("Layers.m_Bits")? as u32;
                // LineRenderer.transform: its object's local TRS and each parent's below the gun's own object.
                let mut at = l.object(field(v, "LineRenderer")?)?.ok_or("TemperatureRayGunBehaviour.LineRenderer is null")?;
                while at != o {
                    x.Barrel.push(inst.objects[at].trs);
                    at = inst.objects[at].parent.ok_or("TemperatureRayGunBehaviour.LineRenderer is outside the gun")?;
                }
                x.line = l.add_line(self, db, field(v, "LineRenderer")?, "TemperatureRayGunBehaviour")?;
                ray_guns.insert(o, self.ray_guns.len());
                self.ray_guns.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "BulbBehaviour") {
                let mut x = super::devices::Bulb::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.Phys = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("BulbBehaviour.PhysicalBehaviour is null")?;
                x.OtherPart = l.object(field(v, "OtherPart")?)?;
                x.Filament = self.sprite_field(&l, v, "Filament")?;
                x.FilamentColour = super::visual::Gradient::read(field(v, "FilamentColour")?)?;
                x.LightSprite = self.sprite_field(&l, v, "LightSprite")?;
                // BrokenSprite on PhysicalBehaviour.spriteRenderer: the renderer of the physical's object.
                if let Some(p) = field(v, "BrokenSprite")?.pptr().filter(|p| !p.is_null()) {
                    let po = *l.arenas.physical.iter().find(|(_, k)| **k == x.Phys).ok_or("BulbBehaviour: the physical's object")?.0;
                    let r = prefab.objects[po].components.iter().find_map(|(_, y)| match y { Component::SpriteRenderer(r) => Some(r), _ => None }).ok_or("BulbBehaviour: the physical has no SpriteRenderer")?;
                    let file = prefab.objects[o].game_object.file;
                    let s = db.resolve(file, p).unwrap_or_else(|| unimplemented!("mod-only: BulbBehaviour.BrokenSprite outside the data"));
                    x.BrokenSprite = Some((s, renderer_bounds(&Sprite::read(db.data(s))?, r)));
                }
                // Explosion.GetComponentInChildren<ExplosionSoundBehviour>(): the object, then its children depth first.
                let explosion = l.object(field(v, "Explosion")?)?.ok_or("BulbBehaviour.Explosion is null")?;
                let mut stack = vec![explosion];
                while let Some(e) = stack.pop() {
                    if let Some((sc, senabled, sv)) = script(prefab, e, "ExplosionSoundBehviour") {
                        let set = field(sv, "audioSource")?.pptr().is_some_and(|p| !p.is_null());
                        let has = set || prefab.objects[e].components.iter().any(|(_, x)| matches!(x, Component::AudioSource { .. }));
                        let near = (senabled && has).then(|| array(sv, "Near").map(<[_]>::len)).transpose()?;
                        x.sound = Some((inst.objects[e].components[sc], near, field(sv, "DistantSound")?.i64().ok_or("DistantSound")?));
                        break;
                    }
                    stack.extend(prefab.objects[e].children.iter().rev());
                }
                if let Some(p) = field(v, "ColliderToDisable")?.pptr().filter(|p| !p.is_null()) {
                    let (co, cc) = prefab.index[&ObjRef { file: l.file, path_id: p.path_id }];
                    if co != o {
                        unimplemented!("mod-only: BulbBehaviour.ColliderToDisable on another object");
                    }
                    x.collider = Some(inst.objects[co].components[cc]);
                }
                x.index = c;
                x.after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
                bulbs.insert(o, self.bulbs.len());
                self.bulbs.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ThrusterbedBehaviour") {
                let mut x = super::thruster::Thrusterbed::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.Phys = *l.arenas.physical.get(&o).ok_or("ThrusterbedBehaviour without a PhysicalBehaviour")?;
                x.LayersToBurn = field(field(v, "LayersToBurn")?, "m_Bits")?.i64().ok_or("LayersToBurn.m_Bits")? as u32;
                x.StartupStrengthCurve = Some(curve(field(v, "StartupStrengthCurve")?)?);
                thrusterbeds.insert(o, self.thrusterbeds.len());
                self.thrusterbeds.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "PistonBehaviour") {
                let mut x = super::piston::Piston::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.Phys = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("PistonBehaviour.PhysicalBehaviour is null")?;
                x.Head = l.in_arena(field(v, "PistonHead")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("PistonBehaviour.PistonHead is null")?;
                let m = l.object(field(v, "DamagableMachinery")?)?.ok_or("PistonBehaviour.DamagableMachinery is null")?;
                x.Machinery = *machinery.get(&m).ok_or("PistonBehaviour: no DamagableMachineryBehaviour on its object")?;
                if l.object(field(v, "Slider")?)? != Some(o) {
                    unimplemented!("mod-only: PistonBehaviour.Slider on another object");
                }
                pistons.insert(o, self.pistons.len());
                self.pistons.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "SmoothLineControllerBehaviour") {
                let mut x = super::smooth_line::SmoothLine::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.line = l.add_line(self, db, field(v, "LineRenderer")?, "SmoothLineControllerBehaviour")?;
                if !self.lines[x.line].use_world_space {
                    unimplemented!("mod-only: SmoothLineControllerBehaviour with a LineRenderer that is not in world space");
                }
                let node = |name: &str| -> Result<super::smooth_line::Node, String> {
                    let n = field(v, name)?;
                    let tf = |f: &str| -> Result<Option<i32>, String> { Ok(l.object(field(n, f)?)?.map(|t| inst.objects[t].components[0])) };
                    let ld = field(n, "LocalDirection")?;
                    Ok(super::smooth_line::Node { transform: tf("Transform")?, direction_target: tf("DirectionTarget")?, local_direction: [float(ld, "x")?, float(ld, "y")?] })
                };
                x.From = node("From")?;
                x.To = node("To")?;
                self.smooth_lines.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "JukeboxBehaviour") {
                let mut x = super::jukebox::Jukebox::default();
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.tracks = audio_clip_names(db, file, v, "tracks")?;
                x.DamageRadius = float(v, "DamageRadius")?;
                x.DamageLayers = field(field(v, "DamageLayers")?, "m_Bits")?.i64().ok_or("DamageLayers.m_Bits")? as u32;
                x.physical = l.in_arena(field(v, "physicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("JukeboxBehaviour.physicalBehaviour is null")?;
                x.renderer = self.sprite_field(&l, v, "renderer")?;
                x.glow = self.sprite_field(&l, v, "glow")?;
                x.glowGradient = super::visual::Gradient::read(field(v, "glowGradient")?)?;
                x.glowGradientSpeed = float(v, "glowGradientSpeed")?;
                jukeboxes.insert(o, self.jukeboxes.len());
                self.jukeboxes.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "AntennaReceiverBehaviour") {
                let mut x = super::antenna::AntennaReceiver::default();
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                // GetComponent<PhysicalBehaviour>() of the receiver (EnergyWireBehaviour's receiver.phys)
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: AntennaReceiverBehaviour without a PhysicalBehaviour (NullReferenceException in EnergyWireBehaviour)"));
                x.TargetPosition = floats(field(v, "TargetPosition")?)?;
                let m = field(v, "TargetMap")?.pptr().ok_or("AntennaReceiverBehaviour.TargetMap")?;
                x.target_map = if m.is_null() { None } else { db.resolve(file, m) };
                antenna_receivers.insert(o, self.antenna_receivers.len());
                self.antenna_receivers.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "AdhesiveCouplerBehaviour") {
                let mut x = super::coupler::Coupler::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.phys = *l.arenas.physical.get(&o).unwrap_or_else(|| unimplemented!("mod-only: AdhesiveCouplerBehaviour without a PhysicalBehaviour"));
                for (name, slot) in [("SlimeSplatPrefab", &mut x.SlimeSplatPrefab), ("GooLineRendererPrefab", &mut x.GooLineRendererPrefab)] {
                    let r = field(v, name)?.pptr().ok_or(name)?;
                    *slot = db.resolve(file, r).filter(|_| !r.is_null()).ok_or_else(|| format!("AdhesiveCouplerBehaviour.{name} null or outside the data"))?;
                }
                x.mask = field(field(v, "LayerMask")?, "m_Bits")?.i64().ok_or("AdhesiveCouplerBehaviour.LayerMask.m_Bits")? as u32;
                Field::set(&mut x.FakeLiquidColor, field(v, "FakeLiquidColor")?)?;
                x.LiquidContainer = l.in_arena(field(v, "LiquidContainer")?, &liquid_containers, "LiquidContainerController")?;
                let kz = l.object(field(v, "KillZone")?)?.unwrap_or_else(|| unimplemented!("mod-only: AdhesiveCouplerBehaviour.KillZone is null"));
                if prefab.objects[kz].components.iter().any(|(_, k)| matches!(k, Component::Rigidbody2D(_) | Component::Collider2D(_) | Component::Joint2D(_))) {
                    unimplemented!("mod-only: AdhesiveCouplerBehaviour.KillZone with physics (its scale)");
                }
                x.KillZone = inst.objects[kz].components[0];
                x.WidthAnimationCurve = Some(curve(field(v, "WidthAnimationCurve")?)?);
                // The goo line prefab's LineRenderer (GetComponent on its root), copied for each Connection.
                let goo = db.prefab(x.GooLineRendererPrefab)?;
                let lc = goo.objects[0].components.iter().position(|(_, k)| matches!(k, Component::Other(120))).ok_or("AdhesiveCouplerBehaviour: no LineRenderer on the GooLineRendererPrefab's root")?;
                let data = crate::assets::components::LineRendererData::read(db.data(goo.objects[0].components[lc].0))?;
                x.goo_line = Some(Game::line_visual(db, x.GooLineRendererPrefab.file, data, 0, BareAnchor::Root, Vec::new(), goo.objects[0].layer as i32, true));
                x.goo_line_at = lc;
                couplers.insert(o, self.couplers.len());
                self.couplers.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "LightningToolEntityBehaviour") {
                let mut x = super::lightning_tool::LightningEntity::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                if !enabled || inst.objects[o].parent.is_some() || l.arenas.physical.contains_key(&o) {
                    unimplemented!("mod-only: LightningToolEntityBehaviour disabled, parented or on a physical object");
                }
                x.thunder = array(v, "Thunder")?.len();
                // LightningToolEntityBehaviour.Awake: vertices = new Vector3[SingleBolt.positionCount] (the LargeFlashLight is rendering).
                let line = field(v, "SingleBolt")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: LightningToolEntityBehaviour.SingleBolt null or outside its prefab (NullReferenceException in Awake)"));
                let (lo, lc) = *prefab.index.get(&ObjRef { file, path_id: line.path_id }).ok_or("LightningToolEntityBehaviour.SingleBolt: dangling reference")?;
                if !matches!(prefab.objects[lo].components[lc].1, Component::Other(120)) {
                    return Err("LightningToolEntityBehaviour.SingleBolt is not a LineRenderer".into());
                }
                x.vertices = crate::assets::components::line_position_count(db.data(prefab.objects[lo].components[lc].0))? as usize;
                x.bolt = Some(l.add_line_on(self, db, lo, "LightningToolEntityBehaviour.SingleBolt")?);
                x.flash = self.sprite_field(&l, v, "LargeFlashLight")?;
                lightning_entities.insert(o, self.lightning_entities.len());
                self.lightning_entities.push(x);
                // Awake: LargeFlashLight.enabled = false.
                if let Some(s) = self.lightning_entities.last().unwrap().flash {
                    self.set_sprite_enabled(s, false);
                }
            }
            if let Some((c, enabled, v)) = script(prefab, o, "PyroPowerToolEntity") {
                let mut x = super::pyro_tool::PyroEntity::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                if !enabled || inst.objects[o].parent.is_some() || l.arenas.physical.contains_key(&o) {
                    unimplemented!("mod-only: PyroPowerToolEntity disabled, parented or on a physical object");
                }
                x.Layers = field(field(v, "Layers")?, "m_Bits")?.i64().ok_or("PyroPowerToolEntity.Layers.m_Bits")? as u32;
                pyro_entities.insert(o, self.pyro_entities.len());
                for (k, n) in super::pyro_tool::PYRO_SYSTEMS.into_iter().enumerate() {
                    self.add_embedded_ps(&l, prefab, inst, field(v, n)?, x.id, super::EmbeddedDrive::Pyro { e: self.pyro_entities.len(), k: k as u8 })?;
                }
                self.pyro_entities.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "SeismicChargeExplosionBehaviour") {
                let mut x = super::seismic_explosion::SeismicExplosion::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                if l.arenas.physical.contains_key(&o) || inst.objects[o].parent.is_some() {
                    unimplemented!("mod-only: SeismicChargeExplosionBehaviour on a moving or parented object (its transform.position)");
                }
                let t = inst.objects[o].trs.t;
                x.position = [t[0], t[1], t[2]];
                x.XScaleCurve = Some(curve(field(v, "XScaleCurve")?)?);
                x.YScaleCurve = Some(curve(field(v, "YScaleCurve")?)?);
                let m = field(v, "Magnet")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: SeismicChargeExplosionBehaviour.Magnet null or outside its prefab (NullReferenceException in FixedUpdate)"));
                let (mo, mc) = *prefab.index.get(&ObjRef { file, path_id: m.path_id }).ok_or("SeismicChargeExplosionBehaviour.Magnet: dangling reference")?;
                if !matches!(prefab.objects[mo].components[mc].1, Component::PointEffector2D(_)) {
                    return Err("SeismicChargeExplosionBehaviour.Magnet is not a PointEffector2D".into());
                }
                x.Magnet = inst.objects[mo].components[mc];
                x.BoltCount = field(v, "BoltCount")?.i64().ok_or("SeismicChargeExplosionBehaviour.BoltCount")? as u32;
                let bolt = field(v, "bolt")?.pptr().ok_or("SeismicChargeExplosionBehaviour.bolt")?;
                if !bolt.is_null() {
                    x.bolt = Some(db.resolve(file, bolt).ok_or("SeismicChargeExplosionBehaviour.bolt outside the data")?);
                }
                // Awake parents the bolts at the object's Transform, which a collider anchors (AttachParent::Bare).
                x.collider = prefab.objects[o].components.iter().position(|(_, k)| matches!(k, Component::Collider2D(_))).map(|k| inst.objects[o].components[k]).unwrap_or_else(|| unimplemented!("mod-only: SeismicChargeExplosionBehaviour on an object without a Collider2D (its bolts' parent)"));
                seismic_explosions.insert(o, self.seismic_explosions.len());
                self.seismic_explosions.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "EnergyVesselLightningBehaviour") {
                let mut x = super::seismic_explosion::EnergyBolt::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                let spark = field(v, "Spark")?.pptr().ok_or("EnergyVesselLightningBehaviour.Spark")?;
                if !spark.is_null() {
                    let r = db.resolve(file, spark).ok_or("EnergyVesselLightningBehaviour.Spark outside the data")?;
                    if db.prefab(r)?.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                        unimplemented!("mod-only: Object.Instantiate of an EnergyVesselLightning Spark prefab with physics");
                    }
                    x.Spark = Some(r);
                }
                x.line = l.add_line_on(self, db, o, "EnergyVesselLightningBehaviour")?;
                x.curve = Some(curve(field(v, "curve")?)?);
                let material = db.resolve(file, field(v, "LightningMaterial")?.pptr().ok_or("LightningMaterial")?);
                self.energy_bolt_awake(x.line, material);
                energy_bolts.insert(o, self.energy_bolts.len());
                self.energy_bolts.push(x);
            }
            if let Some((_, _, v)) = script(prefab, o, "TextMeshProControlReplacer") {
                // A world TextMeshPro only: a TextMeshProUGUI is screen-space.
                if let Ok(text) = self.text_field(&l, v, "Text") {
                    let str_of = |e: &Value, n: &str| match field(e, n) {
                        Ok(crate::assets::mono::Value::Str(s)) => Ok(s.clone()),
                        _ => Err(format!("TextMeshProControlReplacer.{n}")),
                    };
                    let pairs = array(v, "ToReplace")?.iter().map(|e| Ok((str_of(e, "ToReplace")?, str_of(e, "ControlCodeName")?))).collect::<Result<Vec<_>, String>>()?;
                    control_replacers.insert(o, self.control_replacers.len());
                    self.control_replacers.push((text, pairs));
                }
            }
            if let Some((c, enabled, v)) = script(prefab, o, "TransmitterBehaviour") {
                let mut x = super::transmitter::Transmitter::default();
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.phys = *l.arenas.physical.get(&o).unwrap_or_else(|| unimplemented!("mod-only: TransmitterBehaviour without a PhysicalBehaviour (Start: NullReferenceException)"));
                x.TextMeshPro = self.text_field(&l, v, "TextMeshPro")?;
                x.Light = self.sprite_field(&l, v, "Light")?;
                x.Channel = match field(v, "Channel")? {
                    crate::assets::mono::Value::Str(s) => s.clone(),
                    _ => return Err("TransmitterBehaviour.Channel".into()),
                };
                // Transmit's SendMessage reaches every component of a transmitter's own GameObject.
                for (_, k) in &prefab.objects[o].components {
                    if let Component::Script { class, .. } = k {
                        let other = class != "TransmitterBehaviour" && class != "PhysicalBehaviour";
                        if other && (super::tool::USE_RECEIVERS.contains(&class.as_str()) || super::continuous::USE_CONTINUOUS_RECEIVERS.contains(&class.as_str())) {
                            unimplemented!("mod-only: TransmitterBehaviour: {class} on its GameObject takes Transmit's SendMessage");
                        }
                    }
                }
                transmitters.insert(o, self.transmitters.len());
                self.transmitters.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ConveyorBeltBehaviour") {
                let mut x = super::conveyor::Conveyor::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("ConveyorBeltBehaviour.PhysicalBehaviour is null")?;
                x.own = l.object_xf(o);
                for wheel in field(v, "WheelJoints")?.array().ok_or("ConveyorBeltBehaviour.WheelJoints")? {
                    let hj = wheel.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: ConveyorBeltBehaviour.WheelJoints entry null or outside its prefab (MissingReferenceException)"));
                    let at = *prefab.index.get(&ObjRef { file, path_id: hj.path_id }).ok_or("ConveyorBeltBehaviour.WheelJoints: dangling reference")?;
                    x.wheels.push(*l.arenas.hinge.get(&at).ok_or("ConveyorBeltBehaviour.WheelJoints: not a HingeJoint2D")?);
                }
                conveyors.insert(o, self.conveyors.len());
                self.conveyors.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ReactorCoreBehaviour") {
                let mut x = super::reactor::ReactorCore::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = *l.arenas.physical.get(&o).ok_or("ReactorCoreBehaviour without a PhysicalBehaviour (Awake: phys.Temperature throws)")?;
                let objects = |name: &str| -> Result<Vec<usize>, String> {
                    array(v, name)?.iter().map(|g| l.object(g)?.ok_or_else(|| format!("ReactorCoreBehaviour.{name}: a null entry (NullReferenceException)"))).collect()
                };
                x.low_alarm = objects("LowTempAlarmGameObjects")?;
                x.high_alarm = objects("HighTempAlarmGameObjects")?;
                x.meltdown_alarm = objects("MeltdownAlarmGameObjects")?;
                for name in ["LowTempAlarmBehaviours", "HighTempAlarmBehaviours", "MeltdownAlarmBehaviours"] {
                    if !array(v, name)?.is_empty() {
                        unimplemented!("mod-only: ReactorCoreBehaviour.{name}: MonoBehaviour.enabled of the alarm behaviours");
                    }
                }
                x.disable_on_meltdown = Some(objects("DisableOnMeltdown")?);
                x.on_meltdown_start = event_calls(prefab, file, field(v, "OnMeltdownStart")?, inst, "ReactorCoreBehaviour.OnMeltdownStart", true)?;
                x.output_nodes = Some(array(v, "OutputNodes")?.iter().map(|n| l.physical(n)?.ok_or_else(|| "ReactorCoreBehaviour.OutputNodes: a null entry".to_string())).collect::<Result<Vec<_>, _>>()?);
                let sets = array(v, "EnergyConduits")?.iter().map(|n| l.object(n)?.ok_or_else(|| "ReactorCoreBehaviour.EnergyConduits: a null entry (NullReferenceException)".to_string())).collect::<Result<Vec<_>, _>>()?;
                reactor_links.push((self.reactor_cores.len(), sets, l.object(field(v, "MeltdownSequence")?)?));
                reactor_cores.insert(o, self.reactor_cores.len());
                self.reactor_cores.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ReactorA5DisplayBehaviour") {
                let mut x = super::reactor::ReactorA5Display::default();
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.TextMesh = self.text_field(&l, v, "TextMesh")?;
                x.DiagnosticsMaterial = field(v, "DiagnosticsMaterial")?.pptr().and_then(|m| db.resolve(file, m));
                x.awake(l.arenas.physical.get(&o).copied());
                let objects = [l.object(field(v, "Core")?)?, l.object(field(v, "Wire1")?)?, l.object(field(v, "Wire2")?)?, l.object(field(v, "Wire3")?)?, l.object(field(v, "Wire4")?)?];
                a5_links.push((self.reactor_a5_displays.len(), objects[0], [objects[1], objects[2], objects[3], objects[4]]));
                reactor_a5_displays.insert(o, self.reactor_a5_displays.len());
                self.reactor_a5_displays.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "ReactorConduitSetBehaviour") {
                let mut x = super::reactor::ReactorConduitSet::default();
                x.id = inst.objects[o].components[c];
                for b in array(v, "NoCollide")? {
                    x.no_collide.push(l.object(b)?.ok_or("ReactorConduitSetBehaviour.NoCollide: a null entry (NullReferenceException in Start)")?);
                }
                reactor_conduit_sets.insert(o, self.reactor_conduit_sets.len());
                self.reactor_conduit_sets.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ReactorConduitBehaviour") {
                let mut x = super::reactor::ReactorConduit::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.object = o;
                x.physical = *l.arenas.physical.get(&o).ok_or("ReactorConduitBehaviour without a PhysicalBehaviour")?;
                let inflow = field(v, "OptionalInflow")?;
                x.inflow_id = string(inflow, "LiquidID")?;
                x.inflow_amount = float(inflow, "Amount")?;
                x.inflow_liquid = super::circulation::LIQUIDS.iter().position(|(n, _)| *n == x.inflow_id);
                x.clips = array(v, "BreakClips")?.len();
                conduit_links.push((self.reactor_conduits.len(), l.object(field(v, "LiquidPushTarget")?)?));
                reactor_conduits.insert(o, self.reactor_conduits.len());
                self.reactor_conduits.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ReactorMeltdownBehaviour") {
                let mut x = super::reactor::ReactorMeltdown::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.SparkHitLayers = field(field(v, "SparkHitLayers")?, "m_Bits")?.i64().ok_or("ReactorMeltdownBehaviour.SparkHitLayers.m_Bits")? as u32;
                reactor_meltdowns.insert(o, self.reactor_meltdowns.len());
                self.reactor_meltdowns.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "PointAtBehaviour") {
                let mut x = super::reactor::PointAt::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                let pp = field(v, "Rigidbody")?.pptr().ok_or("Rigidbody")?;
                let (ro, rc) = prefab.index.get(&ObjRef { file, path_id: pp.path_id }).copied().filter(|_| !pp.is_null() && pp.file == 0).ok_or("PointAtBehaviour.Rigidbody outside the prefab or null (NullReferenceException)")?;
                x.rigidbody = inst.objects[ro].components[rc];
                x.target = l.physical(field(v, "Target")?)?.ok_or("PointAtBehaviour.Target is null or has no PhysicalBehaviour")?;
                point_ats.insert(o, self.point_ats.len());
                self.point_ats.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "VacuumComponent") {
                let mut x = super::reactor::Vacuum::default();
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                let pp = field(v, "Collider")?.pptr().ok_or("Collider")?;
                let (to, tc) = prefab.index.get(&ObjRef { file, path_id: pp.path_id }).copied().filter(|_| !pp.is_null() && pp.file == 0).ok_or("VacuumComponent.Collider outside the prefab or null")?;
                x.fixtures = inst.fixtures.get(&(to, tc)).cloned().unwrap_or_default();
                let filter = field(v, "Filter")?;
                let flag = |name: &str| field(filter, name).and_then(|x| x.bool().ok_or_else(|| name.to_string()));
                if !flag("useTriggers")? || flag("useDepth")? || flag("useOutsideDepth")? || flag("useNormalAngle")? || flag("useOutsideNormalAngle")? {
                    unimplemented!("mod-only: VacuumComponent.Filter: useTriggers off, depth or normal angle filtering");
                }
                if flag("useLayerMask")? {
                    x.mask = field(field(filter, "layerMask")?, "m_Bits")?.i64().ok_or("layerMask.m_Bits")? as u32;
                }
                vacuums.insert(o, self.vacuums.len());
                self.vacuums.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "AcidPoolBehaviour") {
                let mut x = super::reactor::AcidPool::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                let pool = l.object(field(v, "Pool")?)?.ok_or("AcidPoolBehaviour.Pool is null (NullReferenceException)")?;
                x.Pool = *waters.get(&pool).ok_or("AcidPoolBehaviour.Pool is no WaterBehaviour")?;
                acid_pools.insert(o, self.acid_pools.len());
                self.acid_pools.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "IndustrialLiftBehaviour") {
                let mut x = super::reactor::IndustrialLift::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                let joint = |name: &str| -> Result<Option<(usize, usize)>, String> {
                    let p = field(v, name)?.pptr().ok_or_else(|| name.to_string())?;
                    if p.is_null() {
                        return Ok(None);
                    }
                    prefab.index.get(&ObjRef { file, path_id: p.path_id }).copied().map(Some).ok_or_else(|| format!("IndustrialLiftBehaviour.{name}: dangling"))
                };
                (x.a, x.b) = (joint("A")?, joint("B")?);
                let range = field(v, "MinMaxDistance")?;
                (x.min, x.max) = (float(range, "x")?, float(range, "y")?);
                industrial_lifts.insert(o, self.industrial_lifts.len());
                self.industrial_lifts.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ep") {
                let mut x = super::egg::Ep::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                let asset = field(v, "ToSpawn")?.pptr().ok_or("ep.ToSpawn")?;
                if !asset.is_null() {
                    let r = db.resolve(file, asset).ok_or("ep.ToSpawn outside the data")?;
                    let (_, mb, av) = db.mono(r)?.ok_or("SpawnableAsset")?;
                    let prefab = db.resolve(r.file, av.get("Prefab").and_then(Value::pptr).ok_or("SpawnableAsset.Prefab")?).ok_or("ep.ToSpawn.Prefab")?;
                    x.to_spawn = Some((prefab, mb.name.clone()));
                }
                eps.insert(o, self.eps.len());
                self.eps.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ReactorA5ArcBehaviour") {
                let mut x = super::arc::ReactorA5Arc::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.electrode = l.object(field(v, "OtherElectrode")?)?.ok_or("ReactorA5ArcBehaviour.OtherElectrode is null (NullReferenceException)")?;
                x.mask = field(field(v, "OverlapMask")?, "m_Bits")?.i64().ok_or("OverlapMask.m_Bits")? as u32;
                for e in array(v, "Exclude")? {
                    let p = e.pptr().ok_or("Exclude entry")?;
                    if p.is_null() {
                        continue;
                    }
                    let (o2, c2) = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).ok_or("ReactorA5ArcBehaviour.Exclude outside the prefab")?;
                    x.exclude.push(inst.objects[o2].components[c2]);
                }
                x.points = array(v, "Points")?.len();
                x.line = l.add_line(self, db, field(v, "LineRenderer")?, "ReactorA5ArcBehaviour")?;
                self.reactor_a5_arcs.push(x);
                self.reactor_a5_arc_start(self.reactor_a5_arcs.len() - 1);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "PhysicalRopeCreator") {
                let mut x = super::rope::PhysicalRopeCreator::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.line = l.add_line(self, db, field(v, "LineRenderer")?, "PhysicalRopeCreator")?;
                x.line_object = l.object(field(v, "LineRenderer")?)?.ok_or("PhysicalRopeCreator.LineRenderer is null (NullReferenceException)")?;
                self.rope_creators.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "NodeBehaviour") {
                let mut x = super::activations::ActivationNode::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.owner = l.arenas.physical.get(&o).map(|&p| BodyOwner::Physical(p)).or_else(|| node_rigidbodies.get(&o).map(|&k| BodyOwner::Node(k)));
                self.activation_nodes.push(x);
            }
            // NewUseWireBehaviour: a wire whose typedJoint is the prefab's own DistanceJoint2D, linked as a CopperWireBehaviour is.
            if let Some((c, _enabled, v)) = script(prefab, o, "NewUseWireBehaviour") {
                let colour = field(v, "WireColor")?;
                let colour = [float(colour, "r")?, float(colour, "g")?, float(colour, "b")?, float(colour, "a")?];
                let name = string(v, "WireMaterialName")?;
                let glow = field(v, "Color")?;
                let x = super::blood_wire::BloodWire {
                    kind: super::blood_wire::WireKind::NewUse,
                    id: inst.objects[o].components[c],
                    physical: *l.arenas.physical.get(&o).ok_or("NewUseWireBehaviour without a PhysicalBehaviour (Awake: GetComponent)")?,
                    joint: usize::MAX,
                    WireWidth: float(v, "WireWidth")?,
                    WireColor: colour,
                    start_color: colour,
                    end_color: colour,
                    Channel: field(v, "Channels")?.i64().ok_or("NewUseWireBehaviour.Channels")? as u16,
                    CanBreak: field(v, "CanBreak")?.bool().ok_or("NewUseWireBehaviour.CanBreak")?,
                    Color: [float(glow, "r")?, float(glow, "g")?, float(glow, "b")?, float(glow, "a")?],
                    // WireBehaviour.Initialise
                    Invisible: colour[3] < 0.01 || name == "DeleteWire",
                    ..super::blood_wire::BloodWire::new(super::blood_wire::WireKind::NewUse)
                };
                copper_wires.insert(o, self.blood_wires.len());
                self.blood_wires.push(x);
            }
            if let Some((c, _enabled, v)) = script(prefab, o, "CopperWireBehaviour") {
                // WireBehaviour's serialized fields; Awake finds the PhysicalBehaviour (the joint is adopted at Awake).
                let colour = field(v, "WireColor")?;
                let colour = [float(colour, "r")?, float(colour, "g")?, float(colour, "b")?, float(colour, "a")?];
                let name = string(v, "WireMaterialName")?;
                let x = super::blood_wire::BloodWire {
                    kind: super::blood_wire::WireKind::Copper,
                    id: inst.objects[o].components[c],
                    physical: *l.arenas.physical.get(&o).ok_or("CopperWireBehaviour without a PhysicalBehaviour (Awake: GetComponent)")?,
                    joint: usize::MAX,
                    WireWidth: float(v, "WireWidth")?,
                    WireColor: colour,
                    start_color: colour,
                    end_color: colour,
                    // WireBehaviour.Initialise
                    Invisible: colour[3] < 0.01 || name == "DeleteWire",
                    ..super::blood_wire::BloodWire::new(super::blood_wire::WireKind::Copper)
                };
                copper_wires.insert(o, self.blood_wires.len());
                self.blood_wires.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "RandomSoundBehaviour") {
                let mut x = super::reactor::RandomSound::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.clips = array(v, "Clips")?.len();
                x.has_source = l.object(field(v, "AudioSource")?)?.is_some();
                x.has_physical = l.arenas.physical.contains_key(&o);
                x.physical = l.arenas.physical.get(&o).copied();
                random_sounds.insert(o, self.random_sounds.len());
                self.random_sounds.push(x);
            }
            if let Some((c, _, _)) = script(prefab, o, "LoopSoundBehaviour") {
                let mut x = super::audio::LoopSound::default();
                x.id = inst.objects[o].components[c];
                loop_sounds.insert(o, self.loop_sounds.len());
                self.loop_sounds.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "JointMotorSoundBehaviour") {
                let mut x = super::audio::JointMotorSound::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                match field(v, "Joint")?.pptr().ok_or("JointMotorSoundBehaviour.Joint")? {
                    p if p.is_null() => {}
                    p => {
                        let at = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).filter(|_| p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: JointMotorSoundBehaviour.Joint outside its prefab"));
                        if let Some(&h) = l.arenas.hinge.get(&at) {
                            x.joint = Some(h);
                        } else {
                            let Component::Joint2D(j) = &prefab.objects[at.0].components[at.1].1 else { unimplemented!("mod-only: JointMotorSoundBehaviour.Joint is no Joint2D") };
                            if anchored_initial_state(j).slider.is_none() {
                                unimplemented!("mod-only: JointMotorSoundBehaviour.Joint: a WheelJoint2D (only a HingeJoint2D or SliderJoint2D is linked)");
                            }
                            x.slider_at = Some(at);
                        }
                    }
                }
                jms_links.push((self.joint_motor_sounds.len(), l.object(field(v, "LoopSoundBehaviour")?)?));
                joint_motor_sounds.insert(o, self.joint_motor_sounds.len());
                self.joint_motor_sounds.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "PlayRandomClipBehaviour") {
                let mut x = super::reactor::PlayRandomClip::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.clips = array(v, "Clips")?.len();
                x.has_source = l.object(field(v, "Source")?)?.is_some();
                x.has_physical = l.object(field(v, "PhysicalBehaviour")?)?.is_some();
                x.physical = l.object(field(v, "PhysicalBehaviour")?)?.and_then(|t| l.arenas.physical.get(&t).copied());
                play_random_clips.insert(o, self.play_random_clips.len());
                self.play_random_clips.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "DoAtTime") {
                let mut x = super::reactor::DoAtTime::default();
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                for e in array(v, "Events")? {
                    let calls = event_calls(prefab, file, field(e, "Actions")?, inst, "DoAtTime.Actions", true)?;
                    x.Events.push(super::reactor::DoAtEvent { Time: float(e, "Time")?, Passed: false, calls });
                }
                do_at_times.insert(o, self.do_at_times.len());
                self.do_at_times.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "WinchBehaviour") {
                let mut x = super::winch::Winch::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("WinchBehaviour.PhysicalBehaviour is null")?;
                if l.object(field(v, "SliderJoint")?)? != Some(o) {
                    unimplemented!("mod-only: WinchBehaviour.SliderJoint on another object");
                }
                // OnEnable before Start leaves the prefab's useMotor (Awake's useMotor = true must find it on).
                if prefab.objects[o].components.iter().any(|(_, k)| matches!(k, Component::Joint2D(j) if matches!(j.kind, crate::assets::components::JointKind::Slider { use_motor: false, .. }))) {
                    unimplemented!("mod-only: WinchBehaviour: a SliderJoint2D with useMotor off in the prefab (Awake turns it on before Start)");
                }
                let wheel = l.object(field(v, "Wheel")?)?.ok_or("WinchBehaviour.Wheel is null")?;
                let physics = |x: usize| prefab.objects[x].components.iter().any(|(_, k)| matches!(k, Component::Rigidbody2D(_) | Component::Collider2D(_) | Component::Joint2D(_)));
                if physics(wheel) || inst.descendants(wheel).into_iter().any(physics) {
                    unimplemented!("mod-only: WinchBehaviour.Wheel with physics (TurnWheel rotates it)");
                }
                x.wheel = inst.objects[wheel].trs;
                x.wheel_obj = wheel;
                x.wheel_refs = self.subtree_renderers(&l, wheel);
                x.PositiveSpeedLight = self.sprite_field(&l, v, "PositiveSpeedLight")?;
                x.NegativeSpeedLight = self.sprite_field(&l, v, "NegativeSpeedLight")?;
                winches.insert(o, self.winches.len());
                self.winches.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "MapLiftBehaviour") {
                let mut x = super::map_lift::MapLift::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.Floors = array(v, "Floors")?.iter().map(|f| f.f32().ok_or("MapLiftBehaviour.Floors element is not a float")).collect::<Result<_, _>>()?;
                let m = l.object(field(v, "damagableMachinery")?)?.ok_or("MapLiftBehaviour.damagableMachinery is null")?;
                x.Machinery = *machinery.get(&m).ok_or("MapLiftBehaviour: no DamagableMachineryBehaviour on its object")?;
                if l.object(field(v, "joint")?)? != Some(o) {
                    unimplemented!("mod-only: MapLiftBehaviour.joint on another object");
                }
                map_lifts.insert(o, self.map_lifts.len());
                self.map_lifts.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "FlamethrowerBehaviour") {
                let mut x = super::flamethrower::Flamethrower::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                // FlamethrowerBehaviour.Awake: phys = GetComponent<PhysicalBehaviour>().
                x.phys = *l.arenas.physical.get(&o).ok_or("FlamethrowerBehaviour without a PhysicalBehaviour")?;
                x.LayerMask = field(field(v, "LayerMask")?, "m_Bits")?.i64().ok_or("LayerMask.m_Bits")? as u32;
                let particle = field(v, "particlePrefab")?.pptr().ok_or("particlePrefab")?;
                if !particle.is_null() {
                    let r = db.resolve(file, particle).ok_or("FlamethrowerBehaviour.particlePrefab outside the data")?;
                    let effect = db.prefab(r)?;
                    if effect.objects[0].tag != super::water::POOLABLE_TAG && effect.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                        unimplemented!("mod-only: FlamethrowerBehaviour: a particlePrefab with physics (its pose)");
                    }
                    x.particlePrefab = Some(r);
                }
                let mut at = l.object(field(v, "Muzzle")?)?.ok_or("FlamethrowerBehaviour.Muzzle is null")?;
                while at != o {
                    x.Muzzle.push(inst.objects[at].trs);
                    at = inst.objects[at].parent.ok_or("FlamethrowerBehaviour.Muzzle is outside the flamethrower")?;
                }
                if x.Muzzle.is_empty() {
                    unimplemented!("mod-only: FlamethrowerBehaviour.Muzzle on the flamethrower's own object");
                }
                flamethrowers.insert(o, self.flamethrowers.len());
                self.flamethrowers.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "LaunchedRocketBehaviour") {
                let mut x = super::rocket::LaunchedRocket::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.ImmobiltyFieldLayer = field(field(v, "ImmobiltyFieldLayer")?, "m_Bits")?.i64().ok_or("ImmobiltyFieldLayer.m_Bits")? as u32;
                let trail = l.object(field(v, "relevantSystem")?)?.unwrap_or_else(|| unimplemented!("mod-only: LaunchedRocketBehaviour.relevantSystem is null (NullReferenceException on a hit)"));
                x.relevantSystem = inst.objects[trail].components[0];
                x.Explosion = Some(explosion_parameters(field(v, "Explosion")?)?);
                rockets.insert(o, self.rockets.len());
                self.rockets.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "EggBehaviour") {
                let file = prefab.objects[o].game_object.file;
                let Some(r) = prefab.objects[o].components.iter().find_map(|(_, x)| if let Component::SpriteRenderer(r) = x { Some(r) } else { None }) else {
                    unimplemented!("mod-only: EggBehaviour without a SpriteRenderer");
                };
                let sprite = db.resolve(file, r.sprite).unwrap_or_else(|| unimplemented!("mod-only: EggBehaviour: a SpriteRenderer without a sprite"));
                let bounds = renderer_bounds(&Sprite::read(db.data(sprite))?, r);
                let number = field(v, "Number")?.i64().ok_or("EggBehaviour.Number")? as i32;
                let text_mesh = self.text_field(&l, v, "TextMesh")?;
                self.eggs.push(super::acts::Egg { id: inst.objects[o].components[c], Number: number, bounds, TextMesh: text_mesh });
            }
            if let Some((c, _, v)) = script(prefab, o, "AirfoilBehaviour") {
                let mut x = super::airfoil::Airfoil::default();
                x.id = inst.objects[o].components[c];
                x.PhysicalBehaviour = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("AirfoilBehaviour.PhysicalBehaviour is null")?;
                // SpriteRenderer: the renderer on the same object, which the physical's sprite is (SetWingType swaps it).
                let file = prefab.objects[o].game_object.file;
                let sr = field(v, "SpriteRenderer")?.pptr().filter(|p| !p.is_null()).ok_or("AirfoilBehaviour.SpriteRenderer is null")?;
                let (so, sc) = prefab.index[&ObjRef { file, path_id: sr.path_id }];
                if so != o {
                    unimplemented!("mod-only: AirfoilBehaviour.SpriteRenderer on another object");
                }
                let r = match &prefab.objects[o].components[sc].1 {
                    Component::SpriteRenderer(r) => r,
                    _ => return Err("AirfoilBehaviour.SpriteRenderer is not a SpriteRenderer".into()),
                };
                for w in array(v, "WingTypes")? {
                    let sprite = match field(w, "Sprite")?.pptr().ok_or("AirfoilBehaviour.WingType.Sprite")? {
                        p if p.is_null() => None,
                        p => {
                            let s = db.resolve(file, p).unwrap_or_else(|| unimplemented!("mod-only: AirfoilBehaviour.WingType: a Sprite outside the data"));
                            Some((s, renderer_bounds(&Sprite::read(db.data(s))?, r)))
                        }
                    };
                    x.WingTypes.push(super::airfoil::WingType {
                        Name: string(w, "Name")?,
                        Sprite: sprite,
                        Lifts: field(w, "Lifts")?.bool().ok_or("AirfoilBehaviour.WingType.Lifts")?,
                        LiftMultiplier: float(w, "LiftMultiplier")?,
                        DragMultiplier: float(w, "DragMultiplier")?,
                    });
                }
                // A serialised CurrentWingType that lifts or drags would act in FixedUpdate before Start replaces it.
                let current = field(v, "CurrentWingType")?;
                if field(current, "Lifts")?.bool() != Some(false) || float(current, "DragMultiplier")? != 0.0 {
                    unimplemented!("mod-only: AirfoilBehaviour: a serialised CurrentWingType that lifts or drags");
                }
                x.CurrentWingTypeName = string(v, "CurrentWingTypeName")?;
                airfoils.insert(o, self.airfoils.len());
                self.airfoils.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "StunnerBehaviour") {
                let mut x = super::stunner::Stunner::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.mask = field(field(v, "mask")?, "m_Bits")?.i64().ok_or("StunnerBehaviour.mask.m_Bits")? as u32;
                let file = prefab.objects[o].game_object.file;
                let impact = field(v, "ImpactEffect")?.pptr().ok_or("StunnerBehaviour.ImpactEffect")?;
                if !impact.is_null() {
                    x.ImpactEffect = Some(db.resolve(file, impact).ok_or("StunnerBehaviour.ImpactEffect outside the data")?);
                }
                stunners.insert(o, self.stunners.len());
                self.stunners.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "SoapBehaviour") {
                let mut x = super::soap::Soap::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.physical = l.arenas.physical.get(&o).copied();
                x.index = c;
                x.after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
                soaps.insert(o, self.soaps.len());
                self.soaps.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "PowerHammerBehaviour") {
                let mut x = super::power_hammer::PowerHammer::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.physical = l.arenas.physical.get(&o).copied();
                x.index = c;
                x.after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
                // powerImpactCollider: a Collider2D of the prefab (possibly on another object), its instance id.
                let file = prefab.objects[o].game_object.file;
                x.powerImpactCollider = match field(v, "powerImpactCollider")?.pptr().filter(|p| !p.is_null()) {
                    None => None,
                    Some(p) => {
                        let (co, cc) = prefab.index[&ObjRef { file, path_id: p.path_id }];
                        Some(inst.objects[co].components[cc])
                    }
                };
                x.PhysicalBehaviour = l.in_arena(field(v, "physicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("PowerHammerBehaviour.physicalBehaviour is null")?;
                x.ImpactClipCount = array(v, "ImpactClips")?.len();
                let effect = field(v, "HammerEffectPrefab")?.pptr().ok_or("HammerEffectPrefab")?;
                x.HammerEffectPrefab = if effect.is_null() { None } else { Some(db.resolve(file, effect).ok_or("PowerHammerBehaviour.HammerEffectPrefab outside the data")?) };
                x.LayersToConsider = field(field(v, "LayersToConsider")?, "m_Bits")?.i64().ok_or("LayersToConsider.m_Bits")? as u32;
                power_hammers.insert(o, self.power_hammers.len());
                self.power_hammers.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "BalloonBehaviour") {
                let mut x = super::balloon::Balloon::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.PhysicalBehaviour = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("BalloonBehaviour.PhysicalBehaviour is null")?;
                x.physical = l.arenas.physical.get(&o).copied();
                x.index = c;
                x.after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
                let effect = l.object(field(v, "PoppedEffect")?)?.ok_or("BalloonBehaviour.PoppedEffect is null")?;
                x.PoppedEffect = inst.objects[effect].components[0];
                balloons.insert(o, self.balloons.len());
                self.balloons.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "PhysicsGunBehaviour") {
                let mut x = super::physics_gun::PhysicsGun::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                // PhysicsGunBehaviour.Awake: phys = GetComponent<PhysicalBehaviour>(); the line is initialised and hidden in awake_behaviours.
                // PhysicsGunBehaviour.Start: SetTarget(Target) with the null Target changes nothing.
                x.physical = *l.arenas.physical.get(&o).ok_or("PhysicsGunBehaviour without a PhysicalBehaviour (GetComponent is null)")?;
                let mut at = l.object(field(v, "LineStart")?)?.ok_or("PhysicsGunBehaviour.LineStart is null")?;
                while at != o {
                    x.LineStart.push(inst.objects[at].trs);
                    at = inst.objects[at].parent.ok_or("PhysicsGunBehaviour.LineStart is outside the gun")?;
                }
                if x.LineStart.is_empty() {
                    unimplemented!("mod-only: PhysicsGunBehaviour.LineStart on the gun's own object");
                }
                let lo = l.object(field(v, "LineRenderer")?)?.ok_or("PhysicsGunBehaviour.LineRenderer is null (NullReferenceException in Awake)")?;
                let (sc, _, _) = script(prefab, lo, "SmoothLineControllerBehaviour").ok_or("PhysicsGunBehaviour.LineRenderer is no SmoothLineControllerBehaviour")?;
                x.line_id = inst.objects[lo].components[sc];
                x.line_object = lo;
                x.LineTarget = l.object(field(v, "LineTarget")?)?.map(|t| inst.objects[t].components[0]);
                x.LayersToHit = field(field(v, "LayersToHit")?, "m_Bits")?.i64().ok_or("PhysicsGunBehaviour.LayersToHit.m_Bits")? as u32;
                x.LayersToGrab = field(field(v, "LayersToGrab")?, "m_Bits")?.i64().ok_or("PhysicsGunBehaviour.LayersToGrab.m_Bits")? as u32;
                if l.in_arena(field(v, "Target")?, &l.arenas.physical, "PhysicalBehaviour")?.is_some() {
                    unimplemented!("mod-only: PhysicsGunBehaviour.Target set in its prefab (Start holds it)");
                }
                physics_guns.insert(o, self.physics_guns.len());
                self.physics_guns.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ActivatorElectrodeBehaviour") {
                let mut x = super::activator_electrode::ActivatorElectrode::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = *l.arenas.physical.get(&o).ok_or("ActivatorElectrodeBehaviour without a PhysicalBehaviour (GetComponent is null)")?;
                x.LayersToConsider = field(field(v, "LayersToConsider")?, "m_Bits")?.i64().ok_or("ActivatorElectrodeBehaviour.LayersToConsider.m_Bits")? as u32;
                for light in array(v, "Lights")? {
                    let t = light.pptr().ok_or("Lights")?;
                    let at = prefab.index.get(&ObjRef { file, path_id: t.path_id }).filter(|_| !t.is_null() && t.file == 0).unwrap_or_else(|| unimplemented!("mod-only: ActivatorElectrodeBehaviour.Lights: a null or outside entry (NullReferenceException)"));
                    x.lights.push(visuals[at]);
                }
                x.EmitterGlow = self.sprite_field(&l, v, "EmitterGlow")?;
                activator_electrodes.insert(o, self.activator_electrodes.len());
                self.activator_electrodes.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "MetronomeBehaviour") {
                let mut x = super::metronome::Metronome::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.phys = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("MetronomeBehaviour.PhysicalBehaviour is null")?;
                x.HandCurve = Some(curve(field(v, "HandCurve")?)?);
                x.TickAngleBound = float(v, "TickAngleBound")?;
                let hand = field(v, "Hand")?.pptr().ok_or("MetronomeBehaviour.Hand")?;
                if !hand.is_null() {
                    let Some(&(ho, _)) = l.prefab.index.get(&ObjRef { file: l.file, path_id: hand.path_id }).filter(|_| hand.file == 0) else {
                        return Err("MetronomeBehaviour.Hand: a reference outside the prefab".into());
                    };
                    x.hand = Some(inst.objects[ho].components[0]);
                }
                // Awake: muteBehaviour = GetComponent<MuteBehaviour>()
                x.mute = script(prefab, o, "MuteBehaviour").map(|(mc, _, _)| inst.objects[o].components[mc]);
                // Its ticks SendMessage IsolatedActivation to this object: a receiver on it is not rewritten.
                if prefab.objects[o].components.iter().any(|(_, comp)| matches!(comp, Component::Script { class, .. } if class == "UseWireBehaviour" || super::translated::receives(class, "IsolatedActivation"))) {
                    unimplemented!("mod-only: MetronomeBehaviour: IsolatedActivation to a receiver on its own object");
                }
                metronomes.insert(o, self.metronomes.len());
                self.metronomes.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ThermometerBehaviour") {
                let mut x = super::thermometer::Thermometer::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.Phys = l.in_arena(field(v, "Phys")?, &l.arenas.physical, "Phys")?;
                x.TextMesh = self.text_field(&l, v, "TextMesh")?;
                x.Light = self.sprite_field(&l, v, "Light")?;
                thermometers.insert(o, self.thermometers.len());
                self.thermometers.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "InfraredThermometer") {
                let mut x = super::thermometer::InfraredThermometer::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = *l.arenas.physical.get(&o).ok_or("InfraredThermometer without a PhysicalBehaviour (NullReferenceException)")?;
                let mut at = l.object(field(v, "RayPoint")?)?.ok_or("InfraredThermometer.RayPoint is null")?;
                while at != o {
                    x.RayPoint.push(inst.objects[at].trs);
                    at = inst.objects[at].parent.ok_or("InfraredThermometer.RayPoint is outside the thermometer")?;
                }
                if x.RayPoint.is_empty() {
                    unimplemented!("mod-only: InfraredThermometer.RayPoint on the thermometer's own object");
                }
                x.LayersToHit = field(field(v, "LayersToHit")?, "m_Bits")?.i64().ok_or("InfraredThermometer.LayersToHit.m_Bits")? as u32;
                x.line = l.add_line(self, db, field(v, "LineRenderer")?, "InfraredThermometer")?;
                x.ValueText = self.text_field(&l, v, "ValueText")?;
                x.UnitText = self.text_field(&l, v, "UnitText")?;
                infrared_thermometers.insert(o, self.infrared_thermometers.len());
                self.infrared_thermometers.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ActivationToggleBehaviour") {
                let mut x = super::activation_toggle::ActivationToggle::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = *l.arenas.physical.get(&o).ok_or("ActivationToggleBehaviour without a PhysicalBehaviour (GetComponent is null)")?;
                x.light = l.object(field(v, "LightObject")?)?.ok_or("ActivationToggleBehaviour.LightObject is null")?;
                // Its ContinuousActivation listener sends IsolatedContinuousActivation to this object: a receiver there is not rewritten.
                if prefab.objects[o].components.iter().any(|(_, comp)| matches!(comp, Component::Script { class, .. } if class == "UseWireBehaviour" || super::translated::receives(class, "IsolatedContinuousActivation"))) {
                    unimplemented!("mod-only: ActivationToggleBehaviour: IsolatedContinuousActivation to a receiver on its own object");
                }
                activation_toggles.insert(o, self.activation_toggles.len());
                self.activation_toggles.push(x);
            }
            // PressureGaugeBehaviour is the same gauge: its ChargeFunction, GetProgress and MaxChargeDisplay are PowerGaugeBehaviour's.
            if let Some((c, _, v, class)) = ["PowerGaugeBehaviour", "PressureGaugeBehaviour", "TemperatureGaugeBehaviour"].into_iter().find_map(|k| script(prefab, o, k).map(|(c, e, v)| (c, e, v, k))) {
                let mut x = super::gauge::PowerGauge::default();
                x.temperature = class == "TemperatureGaugeBehaviour";
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                let file = prefab.objects[o].game_object.file;
                let label = field(v, "GaugeLabelPrefab")?.pptr().ok_or("GaugeLabelPrefab")?;
                x.label = if label.is_null() { None } else { Some(db.resolve(file, label).ok_or("GaugeBehaviour.GaugeLabelPrefab outside the data")?) };
                x.center = l.object(field(v, "GaugeCenter")?)?.ok_or("GaugeBehaviour.GaugeCenter is null")?;
                (x.center_anchor, x.center_chain) = l.anchor_chain(x.center);
                x.PhysicalBehaviour = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?;
                x.arm = l.object(field(v, "Arm")?)?.ok_or("GaugeBehaviour.Arm is null")?;
                x.damagableMachinery = machinery.get(&o).copied();
                power_gauges.insert(o, self.power_gauges.len());
                self.power_gauges.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "TorchBehaviour") {
                let mut x = super::torch::Torch::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.arenas.physical.get(&o).copied();
                x.index = c;
                x.after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
                x.colliders = prefab.objects[o].components.iter().enumerate().filter(|(_, (_, comp))| matches!(comp, Component::Collider2D(_))).map(|(k, _)| inst.objects[o].components[k]).collect();
                torches.insert(o, self.torches.len());
                self.torches.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "FlareBehaviour") {
                let mut x = super::flare::Flare::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                let file = prefab.objects[o].game_object.file;
                let torch = field(v, "TorchBehaviour")?.pptr().filter(|p| !p.is_null()).ok_or("FlareBehaviour.TorchBehaviour is null")?;
                let (to, tc) = prefab.index[&ObjRef { file, path_id: torch.path_id }];
                x.torch = inst.objects[to].components[tc];
                x.FlareAudioSource = audio_source_field(prefab, inst, file, v, "FlareBehaviour", "FlareAudioSource")?;
                x.FlareTopSprite = self.sprite_field(&l, v, "FlareTopSprite")?.unwrap_or_else(|| unimplemented!("mod-only: FlareBehaviour.FlareTopSprite null (NullReferenceException in Use)"));
                flares.insert(o, self.flares.len());
                self.flares.push(x);
            }
        }
        let mut fire_maps = HashMap::new();
        for o in 0..n {
            let Some((_, _, v)) = script(prefab, o, "LocalFirePropagationMap") else { continue };
            let p = *l.arenas.physical.get(&o).unwrap_or_else(|| unimplemented!("mod-only: LocalFirePropagationMap without a PhysicalBehaviour (NullReferenceException)"));
            let mut m = super::fire::FireMap::new(p);
            m.GridStep = float(v, "GridStep")?;
            m.FireIntensityDampening = float(v, "FireIntensityDampening")?;
            m.FireSpreadSpeed = float(v, "FireSpreadSpeed")?;
            m.Randomisation = float(v, "Randomisation")?;
            m.CalculationOffset = floats(field(v, "CalculationOffset")?)?;
            let pp = field(v, "Collider")?.pptr().ok_or("Collider")?;
            if !pp.is_null() {
                let (co, cc) = prefab.index.get(&ObjRef { file, path_id: pp.path_id }).copied().filter(|_| pp.file == 0).ok_or("LocalFirePropagationMap.Collider outside the prefab")?;
                m.collider = Some(inst.objects[co].components[cc]);
            }
            for x in array(v, "Points")? {
                let neighbours = array(x, "NeighbourIndices")?.iter().map(|i| i.i64().map(|i| i as i32).ok_or("NeighbourIndices")).collect::<Result<_, _>>()?;
                m.Points.push(super::fire::BurnPoint { LocalPoint: floats(field(x, "LocalPoint")?)?, FireIntensity: float(x, "FireIntensity")?, Softness: float(x, "Softness")?, NeighbourIndices: neighbours });
            }
            self.physicals[p].fire_map = Some(self.fire_maps.len());
            fire_maps.insert(o, self.fire_maps.len());
            self.fire_maps.push(m);
        }
        let (mut lavas, mut spawners, mut heat_rects) = (HashMap::new(), HashMap::new(), HashMap::new());
        for o in 0..n {
            if let Some((_, _, v)) = script(prefab, o, "LavaBehaviour") {
                let mut lava = super::scenery::Lava::default();
                load_values(lava.fields(), v)?;
                let pp = field(v, "Trigger")?.pptr().ok_or("Trigger")?;
                let (to, tc) = prefab.index.get(&ObjRef { file, path_id: pp.path_id }).copied().filter(|_| !pp.is_null() && pp.file == 0).ok_or("LavaBehaviour.Trigger outside the prefab or null")?;
                lava.trigger_fixtures = inst.fixtures.get(&(to, tc)).cloned().unwrap_or_default();
                let filter = field(v, "ContactFilter")?;
                let flag = |name: &str| field(filter, name).and_then(|x| x.bool().ok_or_else(|| name.to_string()));
                if flag("useDepth")? || flag("useOutsideDepth")? || flag("useNormalAngle")? || flag("useOutsideNormalAngle")? {
                    unimplemented!("mod-only: LavaBehaviour.ContactFilter depth or normal angle filtering");
                }
                if flag("useLayerMask")? {
                    lava.layer_mask = field(field(filter, "layerMask")?, "m_Bits")?.i64().ok_or("layerMask.m_Bits")? as u32;
                }
                if let Some(r) = field(v, "ImpactPrefab")?.pptr().and_then(|pp| db.resolve(file, pp)) {
                    lava.impact = Some(self.splash_pool_for(db, r)?);
                }
                // LavaBehaviour.Awake: lavas.Add(this).
                // LavaBehaviour.OnDestroy: lavas.Remove(this) is the object's behaviours leaving the Fixed list (see lava_at_point).
                lavas.insert(o, self.lavas.len());
                self.lavas.push(lava);
            }
            if let Some((c, _, v)) = script(prefab, o, "RandomPrefabSpawnerBehaviour") {
                let mut s = super::scenery::Spawner::default();
                load_values(s.fields(), v)?;
                s.id = inst.objects[o].components[c];
                for p in array(v, "Prefabs")? {
                    s.prefabs.push(db.resolve(file, p.pptr().ok_or("Prefabs")?));
                }
                s.chain = inst.chain(o);
                spawners.insert(o, self.spawners.len());
                self.spawners.push(s);
            }
            if let Some((_, _, v)) = script(prefab, o, "TemperatureTargetRectangleBehaviour") {
                // TemperatureTargetRectangleBehaviour.Start: Grid = AmbientTemperatureGridBehaviour.Instance is `Game::ambient_grid`
                let mut h = super::scenery::HeatRect::default();
                load_values(h.fields(), v)?;
                let ints = |name: &str| -> Result<[i32; 3], String> {
                    let x = field(v, name)?;
                    let c = |k: &str| field(x, k).and_then(|y| y.i64().map(|i| i as i32).ok_or_else(|| format!("{name}.{k}")));
                    Ok([c("x")?, c("y")?, c("z")?])
                };
                (h.Size, h.Offset, h.position) = (ints("Size")?, ints("Offset")?, inst.position(o));
                heat_rects.insert(o, self.heat_rects.len());
                self.heat_rects.push(h);
            }
        }
        let mut random_sprites = HashMap::new();
        for o in 0..n {
            let Some((rc, _, v)) = script(prefab, o, "RandomSpriteBehaviour") else { continue };
            let physical = *l.arenas.physical.get(&o).unwrap_or_else(|| unimplemented!("mod-only: RandomSpriteBehaviour without a PhysicalBehaviour"));
            let r = prefab.objects[o].components.iter().find_map(|(_, x)| match x {
                Component::SpriteRenderer(r) => Some(r),
                _ => None,
            });
            let r = r.ok_or("RandomSpriteBehaviour without a SpriteRenderer")?;
            let mut sprites = Vec::new();
            for s in array(v, "sprites")? {
                let s = db.resolve(file, s.pptr().ok_or("sprites")?).unwrap_or_else(|| unimplemented!("mod-only: RandomSpriteBehaviour: a null sprite"));
                sprites.push((s, renderer_bounds(&Sprite::read(db.data(s))?, r)));
            }
            random_sprites.insert(o, self.random_sprites.len());
            self.random_sprites.push(super::misc::RandomSprite {
                PhysicalBehaviour: physical,
                PickRandomOnStart: field(v, "PickRandomOnStart")?.bool().ok_or("PickRandomOnStart")?,
                chosenIndex: field(v, "chosenIndex")?.i64().ok_or("chosenIndex")? as i32,
                sprites,
                listeners: Vec::new(),
                id: inst.objects[o].components[rc],
            });
        }
        for o in 0..n {
            if let Some((c, enabled, v)) = script(prefab, o, "DeterminedAnomaly") {
                let mut x = super::determined_anomaly::DeterminedAnomaly::default();
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.arenas.physical.get(&o).copied();
                x.Prefab = db.resolve(file, field(v, "Prefab")?.pptr().ok_or("Prefab")?);
                determined_anomalies.insert(o, self.determined_anomalies.len());
                self.determined_anomalies.push(x);
            }
            if let Some((_, _, v)) = script(prefab, o, "RandomSpriteChildBehaviour") {
                // Awake: GetComponentInParent<RandomSpriteBehaviour>() (the object itself first).
                let mut at = Some(o);
                let mut parent = None;
                while let Some(x) = at {
                    if let Some(&r) = random_sprites.get(&x) {
                        parent = Some(r);
                        break;
                    }
                    at = prefab.objects[x].parent;
                }
                random_sprite_children.insert(o, self.random_sprite_children.len());
                let sprites = array(v, "Sprites")?.iter().map(|s| s.pptr().and_then(|s| db.resolve(file, s))).collect::<Vec<_>>();
                let renderer = prefab.objects[o].components.iter().position(|(_, x)| matches!(x, Component::SpriteRenderer(_))).map_or(0, |k| inst.objects[o].components[k]);
                self.random_sprite_children.push(super::misc::RandomSpriteChild { SpriteCount: sprites.len(), parent, sprite: None, sprites, renderer });
            }
        }
        for o in 0..n {
            for (c, (_, comp)) in prefab.objects[o].components.iter().enumerate() {
                if let Component::Script { class, fields: v, .. } = comp {
                    if class == "NoCollideGm" {
                        self.no_collide_gms.push(super::car::NoCollideGm { id: inst.objects[o].components[c], a: l.object(field(v, "A")?)?, b: l.object(field(v, "B")?)? });
                    }
                }
            }
        }
        for o in 0..n {
            let Some((c, _, v)) = script(prefab, o, "RandomCarTextureBehaviour") else { continue };
            let mut slots = Vec::new();
            let mut renderers = Vec::new();
            for name in ["Body", "Background", "Bonnet", "Boot", "FrontDoor", "BackDoor"] {
                let r = field(v, name)?.pptr().ok_or(name)?;
                let (ro, rc) = *prefab.index.get(&ObjRef { file, path_id: r.path_id }).filter(|_| r.file == 0).ok_or("RandomCarTextureBehaviour: a renderer outside the prefab")?;
                let Component::SpriteRenderer(data) = &prefab.objects[ro].components[rc].1 else { return Err(format!("RandomCarTextureBehaviour.{name}: not a SpriteRenderer")) };
                slots.push(match l.arenas.physical.get(&ro) {
                    Some(&p) => super::car::CarSlot::Physical(p),
                    None => super::car::CarSlot::Renderer(inst.objects[ro].components[rc]),
                });
                renderers.push(data);
            }
            let mut textures = Vec::new();
            for t in array(v, "Textures")? {
                let mut sprites = Vec::new();
                for (name, r) in ["Body", "Background", "Bonnet", "Boot", "FrontDoor", "BackDoor"].into_iter().zip(&renderers) {
                    let s = db.resolve(file, field(t, name)?.pptr().ok_or(name)?).unwrap_or_else(|| unimplemented!("mod-only: RandomCarTextureBehaviour: a null sprite"));
                    sprites.push((s, renderer_bounds(&Sprite::read(db.data(s))?, r)));
                }
                textures.push(sprites.try_into().unwrap());
            }
            self.random_car_textures.push(super::car::RandomCarTexture {
                id: inst.objects[o].components[c],
                chosenIndex: field(v, "chosenIndex")?.i64().ok_or("chosenIndex")? as i32,
                slots: slots.try_into().unwrap(),
                textures,
                physical: l.arenas.physical.get(&o).copied(),
            });
            self.script_slots.insert(inst.objects[o].components[c], super::save_slots::Slot::RandomCarTextures(self.random_car_textures.len() - 1));
        }
        let gforces = (0..n).filter(|&o| script(prefab, o, "GForceMeasureBehaviour").is_some()).count();
        if gforces != 0 && gforces != l.arenas.limb.len() {
            unimplemented!("mod-only: a prefab with GForceMeasureBehaviour on some limbs only");
        }
        for o in 0..n {
            let Some((_, _, v)) = script(prefab, o, "GForceMeasureBehaviour") else { continue };
            let mut gf = GForce::default();
            load_values(gf.fields(), v)?;
            self.gforces.push(gf);
        }

        if let Some((c, _, v)) = script(prefab, 0, "PersonBehaviour") {
            let mut p = Person { name: prefab.objects[0].name.clone(), id: inst.objects[0].components[c], ..Default::default() };
            load_values(p.fields(), v)?;
            p.Limbs = l.limbs(v, "Limbs")?;
            p.BoneBreakClips = array(v, "BoneBreakClips")?.len();
            for (i, name) in ["DefaultMaterial", "LegacyMaterial", "GorelessMaterial"].into_iter().enumerate() {
                p.materials[i] = field(v, name)?.pptr().and_then(|m| db.resolve(file, m));
            }
            p.DismembermentClips = array(v, "DismembermentClips")?.len();
            if let Some(effect) = field(v, "PoolableImpactEffect")?.pptr().filter(|p| !p.is_null()) {
                p.PoolableImpactEffect = Some(Some(db.resolve(file, effect).ok_or("PoolableImpactEffect outside the data")?));
            }
            // CirculationBehaviour copies the BleedingParticle prefab's DripSounds (a serialized array, never null).
            if let Some(bp) = field(v, "BleedingParticlePrefab")?.pptr().filter(|p| !p.is_null()) {
                let go = db.resolve(file, bp).ok_or("BleedingParticlePrefab outside the data")?;
                p.BleedingParticlePrefab = Some(go);
                let (drips, puff) = bleeding_drip_sounds(db, go)?;
                p.BleedingDripSounds = Some(drips);
                p.BleedingWaterPuff = puff;
            }
            let s = inst.objects[0].trs.s;
            p.localScale = [s[0], s[1], s[2]];
            for pose in array(v, "Poses")? {
                let mut angles = Vec::new();
                for a in array(pose, "Angles")? {
                    let animated = field(a, "Animated")?.bool().ok_or("Animated")?;
                    angles.push(LimbPose {
                        Limb: l.limb(field(a, "Limb")?)?.ok_or("a LimbPose without its limb")?,
                        Animated: animated,
                        PoseRigidityModifier: float(a, "PoseRigidityModifier")?,
                        Angle: float(a, "Angle")?,
                        StartAngle: float(a, "StartAngle")?,
                        EndAngle: float(a, "EndAngle")?,
                        AnimationDuration: float(a, "AnimationDuration")?,
                        RandomInfluence: float(a, "RandomInfluence")?,
                        RandomSpeed: float(a, "RandomSpeed")?,
                        TimeOffset: float(a, "TimeOffset")?,
                        AnimationCurve: Some(curve(field(a, "AnimationCurve")?)?),
                        cache: CurveCache::default(),
                    });
                }
                p.Poses.push(RagdollPose {
                    Name: string(pose, "Name")?,
                    Rigidity: float(pose, "Rigidity")?,
                    ForceMultiplier: float(pose, "ForceMultiplier")?,
                    ShouldStandUpright: field(pose, "ShouldStandUpright")?.bool().ok_or("ShouldStandUpright")?,
                    DragInfluence: float(pose, "DragInfluence")?,
                    UprightForceMultiplier: float(pose, "UprightForceMultiplier")?,
                    ShouldStumble: field(pose, "ShouldStumble")?.bool().ok_or("ShouldStumble")?,
                    State: field(pose, "State")?.i64().ok_or("State")? as i32,
                    AnimationSpeedMultiplier: float(pose, "AnimationSpeedMultiplier")?,
                    Angles: angles,
                });
            }
            self.people.push(p);
        }
        // The translated classes (tools/translator): each component with its plain fields, then its links.
        let mut translated = HashMap::new();
        let physics = |o: usize| std::iter::once(o).chain(inst.descendants(o)).any(|d| prefab.objects[d].components.iter().any(|(_, x)| matches!(x, Component::Collider2D(_) | Component::Rigidbody2D(_))));
        for o in 0..n {
            for (c, (_, comp)) in prefab.objects[o].components.iter().enumerate() {
                let Component::Script { class, enabled, fields: v, .. } = comp else { continue };
                if !super::translated::CLASSES.contains(&class.as_str()) {
                    continue;
                }
                let b = self.tr_new(class, v, inst.objects[o].components[c], *enabled, l.arenas.physical.get(&o).copied(), physics(o))?;
                translated.insert((o, c), b);
                if b.has_collision() {
                    // SendMessage of a collision goes to the GameObject of the PhysicalBehaviour that took it.
                    let physical = *l.arenas.physical.get(&o).unwrap_or_else(|| unimplemented!("mod-only: {class}: collision messages on a GameObject without a PhysicalBehaviour"));
                    if l.arenas.limb.contains_key(&o) {
                        unimplemented!("mod-only: {class} on a limb: SendMessage order with LimbBehaviour");
                    }
                    let after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
                    self.tr.collision_scripts.push(super::translated::rt::CollisionScript { physical, index: c, after_physical, b });
                }
                let trigger = ["OnTriggerEnter2D", "OnTriggerStay2D", "OnTriggerExit2D"].iter().any(|m| super::translated::receives(class, m));
                let joint_break = super::translated::receives(class, "OnJointBreak2D");
                if trigger || joint_break {
                    // SendMessage goes to the receiving GameObject's scripts: select it by that object's components.
                    let ids = |f: fn(&Component) -> bool| -> Vec<i32> { (0..prefab.objects[o].components.len()).filter(|&k| f(&prefab.objects[o].components[k].1)).map(|k| inst.objects[o].components[k]).collect() };
                    let after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
                    if trigger && script(prefab, o, "WaterBehaviour").is_some() {
                        unimplemented!("mod-only: {class} next to a WaterBehaviour: OnTrigger*2D component order");
                    }
                    self.message_scripts.push(super::messages::MessageScript {
                        colliders: ids(|x| matches!(x, Component::Collider2D(_))),
                        rigidbodies: ids(|x| matches!(x, Component::Rigidbody2D(_))),
                        joints: ids(|x| matches!(x, Component::Joint2D(_))),
                        index: c,
                        after_physical,
                        b,
                        trigger,
                        joint_break,
                    });
                }
            }
        }
        for (&(o, c), &b) in &translated {
            use super::translated::{LinkKind as K, LinkVal as V};
            let Component::Script { class, fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
            for &(name, kind) in super::translated::link_fields(class) {
                let x = field(v, name)?;
                // A HingeJoint2D of this prefab: its `Game::hinges` index.
                let hinge = |x: &Value| -> Result<Option<usize>, String> {
                    match x.pptr().ok_or(name)? {
                        p if p.is_null() => Ok(None),
                        p => {
                            let at = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).filter(|_| p.file == 0).ok_or_else(|| format!("{class}.{name} outside the prefab"))?;
                            Ok(Some(*l.arenas.hinge.get(&at).ok_or_else(|| format!("{class}.{name}: not a HingeJoint2D"))?))
                        }
                    }
                };
                let val = match kind {
                    K::Physical => V::Physical(l.in_arena(x, &l.arenas.physical, "PhysicalBehaviour")?),
                    K::Rigidbody => V::Physical(match l.object(x)? {
                        None => None,
                        Some(t) => Some(*l.arenas.physical.get(&t).unwrap_or_else(|| unimplemented!("mod-only: {class}.{name}: a Rigidbody2D without a PhysicalBehaviour"))),
                    }),
                    K::Script(want) => V::Script(match x.pptr().ok_or(name)? {
                        p if p.is_null() => None,
                        p => {
                            let at = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).filter(|_| p.file == 0).ok_or_else(|| format!("{class}.{name} outside the prefab"))?;
                            let t = *translated.get(&at).ok_or_else(|| format!("{class}.{name}: not a translated component"))?;
                            if t.class() != want {
                                return Err(format!("{class}.{name}: a {} for a {want}", t.class()));
                            }
                            Some(t)
                        }
                    }),
                    // An inline value (a Gradient, an AnimationCurve) is never null.
                    K::Present => {
                        if let Some(&(po, pc)) = x.pptr().filter(|p| p.file == 0 && !p.is_null()).and_then(|p| prefab.index.get(&ObjRef { file, path_id: p.path_id })) {
                            self.present_ids.insert((inst.objects[o].components[c], name), inst.objects[po].components[pc]);
                        }
                        V::Present(x.pptr().is_none_or(|p| !p.is_null()))
                    }
                    K::Len => V::Len(x.array().ok_or(name)?.len()),
                    K::Prefab => V::Prefab(match x.pptr().ok_or(name)? {
                        p if p.is_null() => None,
                        // An object of this prefab: by its Transform's instance id. Another prefab of the same file is an asset.
                        p if p.file == 0 && prefab.index.contains_key(&ObjRef { file, path_id: p.path_id }) => Some(super::translated::rt::Go::Local(inst.objects[l.object_of(p)?.unwrap()].components[0])),
                        p => Some(super::translated::rt::Go::Asset(db.resolve(file, p).ok_or_else(|| format!("{class}.{name} outside the data"))?)),
                    }),
                    K::Mask => V::Mask(field(x, "m_Bits")?.i64().ok_or("m_Bits")? as u32),
                    K::Hinge => V::Hinge(hinge(x)?),
                    K::Collider => V::Collider(match x.pptr().ok_or(name)? {
                        p if p.is_null() => 0,
                        p => {
                            let (o, c) = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).filter(|_| p.file == 0).ok_or_else(|| format!("{class}.{name} outside the prefab"))?;
                            if !matches!(prefab.objects[o].components[c].1, Component::Collider2D(_)) {
                                return Err(format!("{class}.{name}: not a Collider2D"));
                            }
                            inst.objects[o].components[c]
                        }
                    }),
                    K::Hinges => V::Hinges(x.array().ok_or(name)?.iter().map(&hinge).collect::<Result<_, _>>()?),
                    K::Event => V::Event(event_calls(prefab, file, x, inst, &format!("{class}.{name}"), true)?),
                    K::Handwritten(arena) => V::Handwritten(match l.object(x)? {
                        None => None,
                        Some(t) => Some(*match arena {
                            "DamagableMachineryBehaviour" => &machinery,
                            "LimbBehaviour" => &l.arenas.limb,
                            _ => unimplemented!("mod-only: {class}.{name}: a {arena}"),
                        }.get(&t).ok_or_else(|| format!("{class}.{name}: no {arena} there"))?),
                    }),
                    // batch3 r5: an object with no physics on or below it by its Transform's id (Xf::Object), as tr_go_transform.
                    K::Transform => V::Transform(l.object(x)?.map(|t| {
                        // (Its Transform's id by the field: TargetLineRenderer.Target, whatever the object holds.)
                        self.present_ids.insert((inst.objects[o].components[c], name), inst.objects[t].components[0]);
                        t
                    }).map(|t| match (l.arenas.physical.get(&t), physics(t)) {
                        (None, false) => super::translated::rt::Xf::Object(inst.objects[t].components[0]),
                        (p, physics) => super::translated::rt::Xf::this(p.copied(), physics),
                    })),
                };
                self.tr_link(b, name, val);
            }
        }
        // The classes that read the translated behaviours: JetEngineBehaviour toggles its GuideTemperatureBehaviour.
        let mut jet_engines = HashMap::new();
        let mut chainsaws = HashMap::new();
        let mut freezes = HashMap::new();
        let mut freeze_serialisations = HashMap::new();
        let mut ceiling_turrets = HashMap::new();
        let mut temperature_transfers = HashMap::new();
        let mut worm_staffs = HashMap::new();
        let mut atom_explosions = HashMap::new();
        let mut prefab_bombs = HashMap::new();
        let mut laser_turrets = HashMap::new();
        let mut sentry_turrets = HashMap::new();
        let mut sentry_guns = HashMap::new();
        let mut ion_bolts = HashMap::new();
        let mut random_events = HashMap::new();
        let mut emps = HashMap::new();
        let mut decimators = HashMap::new();
        let mut drones = HashMap::new();
        let mut portals = HashMap::new();
        let mut black_holes = HashMap::new();
        let mut immobility_fields = HashMap::new();
        let mut tesla_coils = HashMap::new();
        let mut tesla_bolt_sources = HashMap::new();
        let mut energy_swords = HashMap::new();
        let mut signal_converters = HashMap::new();
        for o in 0..n {
            if let Some((c, _, v)) = script(prefab, o, "JetEngineBehaviour") {
                let mut x = super::jet_engine::JetEngine::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.PhysicalBehaviour = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("JetEngineBehaviour.PhysicalBehaviour is null")?;
                let m = l.object(field(v, "DamagableMachinery")?)?.ok_or("JetEngineBehaviour.DamagableMachinery is null")?;
                x.DamagableMachinery = *machinery.get(&m).ok_or("JetEngineBehaviour: no DamagableMachineryBehaviour on its object")?;
                let effector = |name: &str| -> Result<(usize, usize), String> {
                    let pp = field(v, name)?.pptr().ok_or_else(|| name.to_string())?;
                    prefab.index.get(&ObjRef { file, path_id: pp.path_id }).copied().filter(|_| !pp.is_null() && pp.file == 0).ok_or_else(|| format!("JetEngineBehaviour.{name} outside the prefab or null"))
                };
                let (io, ic) = effector("IntakeEffector")?;
                let (eo, ec) = effector("ExhaustEffector")?;
                for (name, k) in [("IntakeEffector", (io, ic)), ("ExhaustEffector", (eo, ec))] {
                    if !matches!(prefab.objects[k.0].components[k.1].1, Component::PointEffector2D(_)) {
                        return Err(format!("JetEngineBehaviour.{name} is not a PointEffector2D"));
                    }
                }
                x.IntakeEffector = inst.objects[io].components[ic];
                x.ExhaustEffector = inst.objects[eo].components[ec];
                // exhaustEffectorCollider = ExhaustEffector.GetComponent<Collider2D>(): the first one.
                let collider = prefab.objects[eo].components.iter().position(|(_, k)| matches!(k, Component::Collider2D(_))).ok_or("JetEngineBehaviour: the ExhaustEffector has no Collider2D")?;
                x.exhaustEffectorCollider = inst.fixtures.get(&(eo, collider)).cloned().unwrap_or_default();
                x.guideTemperatureBehaviour = translated.iter().find(|((to, tc), _)| *to == o && matches!(&prefab.objects[*to].components[*tc].1, Component::Script { class, .. } if class == "GuideTemperatureBehaviour")).map(|(_, &b)| b);
                x.PlasmaJet = self.sprite_field(&l, v, "PlasmaJet")?;
                x.Lighting = self.sprite_field(&l, v, "Lighting")?;
                x.StartupIntensityCurve = Some(curve(field(v, "StartupIntensityCurve")?)?);
                jet_engines.insert(o, self.jet_engines.len());
                self.jet_engines.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "TeslaCoilBehaviour") {
                let mut x = super::tesla_coil::TeslaCoil::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "physicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.unwrap_or_else(|| unimplemented!("mod-only: TeslaCoilBehaviour.physicalBehaviour null (NullReferenceException in CalculateWhReady)"));
                x.audio_null = l.object(field(v, "audioSource")?)?.is_none();
                x.Wh = db.resolve(file, field(v, "Wh")?.pptr().ok_or("Wh")?);
                x.colliders = prefab.objects[o].components.iter().enumerate().filter(|(_, (_, k))| matches!(k, Component::Collider2D(_))).map(|(k, _)| inst.objects[o].components[k]).collect();
                x.sparkClips = array(v, "sparkClips")?.len();
                let p = field(v, "bolt")?.pptr().ok_or("bolt")?;
                let mut source = super::tesla_coil::BoltSource::default();
                source.null = p.is_null();
                if let Some(r) = db.resolve(file, p) {
                    let bolt = db.prefab(r)?;
                    source.ids = bolt.objects.iter().map(|x| 1 + x.components.len()).sum();
                    let k = bolt.objects[0].components.iter().position(|(_, k)| matches!(k, Component::Script { class, .. } if class == "TeslaCoilLightningBoltBehaviour")).ok_or("TeslaCoilBehaviour.bolt without a TeslaCoilLightningBoltBehaviour")?;
                    source.script_at = 1 + k;
                    let Component::Script { execution_order, fields: bv, .. } = &bolt.objects[0].components[k].1 else { unreachable!() };
                    // TeslaCoilLightningBoltBehaviour.Awake: the line (useWorldSpace, LightningMaterial and 64 points) is set in tesla_coil_awake.
                    // Awake sets GetComponent<TeslaCoilLightningBoltBehaviour>().parent; nothing else on the clone runs.
                    if bolt.objects.iter().any(|x| x.components.iter().any(|(_, k)| matches!(k, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                        unimplemented!("mod-only: TeslaCoilBehaviour: a bolt prefab with physics");
                    }
                    let b = &mut source.bolt;
                    b.order = *execution_order;
                    b.seed = float(bv, "seed")?;
                    b.life = float(bv, "life")?;
                    b.Radius = float(bv, "Radius")?;
                    b.layer = field(field(bv, "layer")?, "m_Bits")?.i64().ok_or("layer.m_Bits")? as u32;
                    b.Spark = db.resolve(r.file, field(bv, "Spark")?.pptr().ok_or("Spark")?);
                    b.curve = Some(curve(field(bv, "curve")?)?);
                    let lc = bolt.objects[0].components.iter().position(|(_, k)| matches!(k, Component::Other(120))).ok_or("TeslaCoilLightningBoltBehaviour: no LineRenderer on its GameObject")?;
                    let data = crate::assets::components::LineRendererData::read(db.data(bolt.objects[0].components[lc].0))?;
                    let mut line = Game::line_visual(db, r.file, data, 0, BareAnchor::Root, Vec::new(), bolt.objects[0].layer as i32, true);
                    line.material = db.resolve(r.file, field(bv, "LightningMaterial")?.pptr().ok_or("LightningMaterial")?);
                    source.line = Some(line);
                    source.line_at = 1 + lc;
                    source.prefab = Some(std::sync::Arc::new(bolt));
                }
                tesla_bolt_sources.insert(o, source);
                tesla_coils.insert(o, self.tesla_coils.len());
                self.tesla_coils.push(x);
            }
            // The bolt prefab spawned on its own (the TelsaCoilLightning catalog item): parent stays null.
            if let Some((c, enabled, bv)) = script(prefab, o, "TeslaCoilLightningBoltBehaviour") {
                if !enabled || l.arenas.physical.contains_key(&o) {
                    unimplemented!("mod-only: TeslaCoilLightningBoltBehaviour disabled or on a physical object");
                }
                let Component::Script { execution_order, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                let mut b = super::tesla_coil::TeslaBolt::default();
                b.id = inst.objects[o].components[c];
                b.order = *execution_order;
                b.active = inst.objects[o].active;
                b.seed = float(bv, "seed")?;
                b.life = float(bv, "life")?;
                b.Radius = float(bv, "Radius")?;
                b.layer = field(field(bv, "layer")?, "m_Bits")?.i64().ok_or("layer.m_Bits")? as u32;
                b.Spark = db.resolve(file, field(bv, "Spark")?.pptr().ok_or("Spark")?);
                b.curve = Some(curve(field(bv, "curve")?)?);
                b.material = db.resolve(file, field(bv, "LightningMaterial")?.pptr().ok_or("LightningMaterial")?);
                b.line = l.add_line_on(self, db, o, "TeslaCoilLightningBoltBehaviour")?;
                self.tesla_bolts.push(b);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ImmobilityFieldBehaviour") {
                let mut x = super::immobility_field::ImmobilityField::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.unwrap_or_else(|| unimplemented!("mod-only: ImmobilityFieldBehaviour.PhysicalBehaviour null (NullReferenceException)"));
                // AddComponent<FrictionJoint2D> and GetComponents<FrictionJoint2D> act on the script's GameObject.
                if l.arenas.physical.get(&o) != Some(&x.physical) {
                    unimplemented!("mod-only: ImmobilityFieldBehaviour on another GameObject than its PhysicalBehaviour (its FrictionJoint2Ds)");
                }
                if prefab.objects[o].components.iter().any(|(_, k)| matches!(k, Component::Joint2D(_))) {
                    unimplemented!("mod-only: ImmobilityFieldBehaviour on a GameObject with a serialized Joint2D (GetComponents<FrictionJoint2D>)");
                }
                x.CaptureObject = l.object(field(v, "CaptureObject")?)?.unwrap_or_else(|| unimplemented!("mod-only: ImmobilityFieldBehaviour.CaptureObject null (NullReferenceException in UpdateActivation)"));
                let p = field(v, "Trigger")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: ImmobilityFieldBehaviour.Trigger null or outside its prefab (NullReferenceException)"));
                let k = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).ok_or("ImmobilityFieldBehaviour.Trigger: dangling reference")?;
                if !matches!(prefab.objects[k.0].components[k.1].1, Component::Collider2D(_)) {
                    return Err("ImmobilityFieldBehaviour.Trigger is not a Collider2D".into());
                }
                x.Trigger = k;
                x.colliders = prefab.objects[o].components.iter().enumerate().filter(|(_, (_, k))| matches!(k, Component::Collider2D(_))).map(|(k, _)| inst.objects[o].components[k]).collect();
                x.rigidbody = prefab.objects[o].components.iter().position(|(_, k)| matches!(k, Component::Rigidbody2D(_))).map_or(0, |k| inst.objects[o].components[k]);
                x.CaptureClips = array(v, "CaptureClips")?.len();
                x.Layers = field(field(v, "Layers")?, "m_Bits")?.i64().ok_or("Layers.m_Bits")? as u32;
                immobility_fields.insert(o, self.immobility_fields.len());
                self.immobility_fields.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ForcefieldGeneratorBehaviour") {
                let mut x = super::forcefield::Forcefield::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                // Awake: rigidbody = GetComponent<Rigidbody2D>() (the PhysicalBehaviour's), shield = transform.GetChild(0).
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: ForcefieldGeneratorBehaviour without a PhysicalBehaviour on its GameObject (NullReferenceException in OnTriggerEnter2D)"));
                x.shield = (0..n).find(|&k| prefab.objects[k].parent == Some(o)).unwrap_or_else(|| unimplemented!("mod-only: ForcefieldGeneratorBehaviour.Awake: GetChild(0) of a childless transform (UnityException)"));
                x.colliders = prefab.objects[o].components.iter().enumerate().filter(|(_, (_, k))| matches!(k, Component::Collider2D(_))).map(|(k, _)| inst.objects[o].components[k]).collect();
                x.rigidbody = prefab.objects[o].components.iter().position(|(_, k)| matches!(k, Component::Rigidbody2D(_))).map_or(0, |k| inst.objects[o].components[k]);
                self.forcefields.push(x);
                self.forcefield_awake(self.forcefields.len() - 1);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "BlackHoleBehaviour") {
                let mut x = super::black_hole::BlackHole::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.layer = prefab.objects[o].layer as usize;
                x.colliders = prefab.objects[o].components.iter().enumerate().filter(|(_, (_, k))| matches!(k, Component::Collider2D(_))).map(|(k, _)| inst.objects[o].components[k]).collect();
                black_holes.insert(o, self.black_holes.len());
                self.black_holes.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "DecimatorBehaviour") {
                let mut x = super::decimator::Decimator::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                match l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")? {
                    Some(p) => x.physical = p,
                    None => x.physical_null = true,
                }
                let p = field(v, "DecimateTrigger")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: DecimatorBehaviour.DecimateTrigger null or outside its prefab (NullReferenceException)"));
                let k = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).ok_or("DecimatorBehaviour.DecimateTrigger: dangling reference")?;
                if !matches!(prefab.objects[k.0].components[k.1].1, Component::Collider2D(_)) {
                    return Err("DecimatorBehaviour.DecimateTrigger is not a Collider2D".into());
                }
                x.DecimateTrigger = k;
                x.colliders = prefab.objects[o].components.iter().enumerate().filter(|(_, (_, k))| matches!(k, Component::Collider2D(_))).map(|(k, _)| inst.objects[o].components[k]).collect();
                x.rigidbody = prefab.objects[o].components.iter().position(|(_, k)| matches!(k, Component::Rigidbody2D(_))).map_or(0, |k| inst.objects[o].components[k]);
                // UpdateActivation plays and stops these unconditionally (a null one throws there).
                for name in ["ParticleSystem", "AudioSource"] {
                    x.players_null |= l.object(field(v, name)?)?.is_none();
                }
                // Instantiate(null) in FixedUpdate and OnTriggerEnter2D throws there: a null prefab stays the null ObjRef.
                for (name, slot) in [("DeleteEffect", &mut x.DeleteEffect), ("BlackHole", &mut x.BlackHole)] {
                    let p = field(v, name)?.pptr().ok_or(name)?;
                    if !p.is_null() {
                        *slot = db.resolve(file, p).ok_or_else(|| format!("DecimatorBehaviour.{name} outside the data"))?;
                    }
                }
                decimators.insert(o, self.decimators.len());
                self.decimators.push(x);
            }
            // The Rigidbody2D on this GameObject: its physical's, else its Transform node's.
            let body_owner = |o: usize| l.arenas.physical.get(&o).map(|&p| BodyOwner::Physical(p)).or_else(|| node_rigidbodies.get(&o).map(|&k| BodyOwner::Node(k)));
            if let Some((c, _, v)) = script(prefab, o, "PortalBehaviour") {
                let component = |name: &str| -> Result<i32, String> {
                    let p = field(v, name)?.pptr().filter(|p| !p.is_null()).unwrap_or_else(|| unimplemented!("mod-only: PortalBehaviour.{name} null (NullReferenceException in Start)"));
                    let k = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).ok_or_else(|| format!("PortalBehaviour.{name}: dangling reference"))?;
                    Ok(inst.objects[k.0].components[k.1])
                };
                portals.insert(o, self.portals.len());
                self.portals.push(super::portal::Portal { id: inst.objects[o].components[c], entry: component("EntryTriggerEvent")?, exit: component("ExitTriggerEvent")?, passing_through: Vec::new() });
            }
            if let Some((c, _, _)) = script(prefab, o, "TriggerEventBehaviour") {
                self.trigger_events.push(super::portal::TriggerEvent {
                    id: inst.objects[o].components[c],
                    colliders: prefab.objects[o].components.iter().enumerate().filter(|(_, (_, k))| matches!(k, Component::Collider2D(_))).map(|(k, _)| inst.objects[o].components[k]).collect(),
                    rigidbody: prefab.objects[o].components.iter().position(|(_, k)| matches!(k, Component::Rigidbody2D(_))).map_or(0, |k| inst.objects[o].components[k]),
                    on_enter: None,
                    on_exit: None,
                });
            }
            if let Some((c, enabled, _)) = script(prefab, o, "PropellerBehaviour") {
                // PropellerBehaviour.Awake: GetComponent<Rigidbody2D>() (the owner below).
                let rigidbody = body_owner(o).unwrap_or_else(|| unimplemented!("mod-only: PropellerBehaviour without a Rigidbody2D (NullReferenceException)"));
                self.propellers.push(super::drone::Propeller { id: inst.objects[o].components[c], enabled, rigidbody, hinge: None });
            }
            if let Some((c, enabled, v)) = script(prefab, o, "DroneBehaviour") {
                let mut x = super::drone::Drone::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                // DroneBehaviour.Awake: GetComponent<PhysicalBehaviour>() below; audioSource.loop = true is drone_awake.
                x.physical = *l.arenas.physical.get(&o).unwrap_or_else(|| unimplemented!("mod-only: DroneBehaviour without a PhysicalBehaviour (NullReferenceException)"));
                let rb = field(v, "rigidbody")?.pptr().ok_or("DroneBehaviour.rigidbody")?;
                if !rb.is_null() {
                    let k = *prefab.index.get(&ObjRef { file, path_id: rb.path_id }).ok_or("DroneBehaviour.rigidbody: dangling reference")?;
                    x.rigidbody = Some(body_owner(k.0).unwrap_or_else(|| unimplemented!("mod-only: DroneBehaviour.rigidbody without a body")));
                }
                for e in array(v, "propellers")? {
                    let p = e.pptr().filter(|p| !p.is_null()).unwrap_or_else(|| unimplemented!("mod-only: DroneBehaviour.propellers: a null entry (NullReferenceException in Start)"));
                    let k = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).ok_or("DroneBehaviour.propellers: dangling reference")?;
                    x.propellers.push(inst.objects[k.0].components[k.1]);
                }
                drones.insert(o, self.drones.len());
                self.drones.push(x);
            }
            if let Some((_, _, v)) = script(prefab, o, "VibrationBehaviour") {
                let mut x = super::centrifuge::Vibration::default();
                load_values(x.fields(), v)?;
                x.physical = *l.arenas.physical.get(&o).ok_or("VibrationBehaviour without a PhysicalBehaviour")?;
                vibrations.insert(o, self.vibrations.len());
                self.vibrations.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "CentrifugeBehaviour") {
                let mut x = super::centrifuge::Centrifuge::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = *l.arenas.physical.get(&o).ok_or("CentrifugeBehaviour without a PhysicalBehaviour")?;
                x.LiquidContainer = l.in_arena(field(v, "LiquidContainer")?, &liquid_containers, "LiquidContainerController")?;
                let m = l.object(field(v, "DamagableMachinery")?)?.ok_or("CentrifugeBehaviour.DamagableMachinery is null")?;
                x.DamagableMachinery = *machinery.get(&m).ok_or("CentrifugeBehaviour: no DamagableMachineryBehaviour on its object")?;
                let vib = l.object(field(v, "VibrationBehaviour")?)?.ok_or("CentrifugeBehaviour.VibrationBehaviour is null")?;
                x.VibrationBehaviour = *vibrations.get(&vib).ok_or("CentrifugeBehaviour: its VibrationBehaviour is linked later (component order)")?;
                x.EnabledLight = self.sprite_field(&l, v, "EnabledLight")?;
                for light in array(v, "LiquidCountLights")? {
                    x.LiquidCountLights.push(match l.object(light)? {
                        None => None,
                        Some(t) => Some(self.sprite_at(&l, t).ok_or("LiquidCountLights: no SpriteRenderer")?),
                    });
                }
                centrifuges.insert(o, self.centrifuges.len());
                self.centrifuges.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "LiquidentifierBehaviour") {
                let mut x = super::liquidentifier::Liquidentifier::default();
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("LiquidentifierBehaviour.PhysicalBehaviour is null")?;
                x.LiquidContainer = l.in_arena(field(v, "LiquidContainer")?, &liquid_containers, "LiquidContainerController")?;
                let clip = field(v, "FlushSound")?.pptr().and_then(|p| db.resolve(file, p)).ok_or("LiquidentifierBehaviour.FlushSound is null")?;
                x.flush_length = crate::assets::classes::AudioClip::read(db.data(clip))?.length;
                x.Clips = array(v, "Clips")?.len();
                x.Lights = array(v, "Lights")?
                    .iter()
                    .map(|r| Ok(match l.object(r)? {
                        None => None,
                        Some(t) => Some(self.sprite_at(&l, t).ok_or("LiquidentifierBehaviour.Lights: no SpriteRenderer")?),
                    }))
                    .collect::<Result<_, String>>()?;
                x.TextMesh = self.text_field(&l, v, "TextMesh")?;
                x.StateMesh = self.text_field(&l, v, "StateMesh")?;
                liquidentifiers.insert(o, self.liquidentifiers.len());
                self.liquidentifiers.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "PulseDrumBehaviour") {
                let mut x = super::pulse_drum::PulseDrum::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("PulseDrumBehaviour.PhysicalBehaviour is null")?;
                x.Layers = field(field(v, "Layers")?, "m_Bits")?.i64().ok_or("PulseDrumBehaviour.Layers.m_Bits")? as u32;
                x.Clips = array(v, "Clips")?.len();
                pulse_drums.insert(o, self.pulse_drums.len());
                self.pulse_drums.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "LiquidDuplicatorBehaviour") {
                let mut x = super::liquid_duplicator::LiquidDuplicator::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.ok_or("LiquidDuplicatorBehaviour.PhysicalBehaviour is null")?;
                let m = l.object(field(v, "DamagableMachinery")?)?.ok_or("LiquidDuplicatorBehaviour.DamagableMachinery is null")?;
                x.DamagableMachinery = *machinery.get(&m).ok_or("LiquidDuplicatorBehaviour: no DamagableMachineryBehaviour on its object")?;
                x.LiquidContainer = l.in_arena(field(v, "LiquidContainer")?, &liquid_containers, "LiquidContainerController")?;
                let clip = field(v, "FlushSound")?.pptr().and_then(|p| db.resolve(file, p)).ok_or("LiquidDuplicatorBehaviour.FlushSound is null")?;
                x.flush_length = crate::assets::classes::AudioClip::read(db.data(clip))?.length;
                liquid_duplicators.insert(o, self.liquid_duplicators.len());
                self.liquid_duplicators.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "ChainsawBehaviour") {
                let mut x = super::chainsaw::Chainsaw::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.unwrap_or_else(|| unimplemented!("mod-only: ChainsawBehaviour.PhysicalBehaviour null (NullReferenceException in FixedUpdate)"));
                let m = l.object(field(v, "DamagableMachinery")?)?.unwrap_or_else(|| unimplemented!("mod-only: ChainsawBehaviour.DamagableMachinery null (NullReferenceException in Start)"));
                x.DamagableMachinery = *machinery.get(&m).ok_or("ChainsawBehaviour: no DamagableMachineryBehaviour on its object")?;
                let sc = field(v, "SawCollider")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: ChainsawBehaviour.SawCollider null or outside its prefab (NullReferenceException in FixedUpdate)"));
                let key = *prefab.index.get(&ObjRef { file, path_id: sc.path_id }).ok_or("ChainsawBehaviour.SawCollider: dangling reference")?;
                if !matches!(prefab.objects[key.0].components[key.1].1, Component::Collider2D(_)) {
                    return Err("ChainsawBehaviour.SawCollider is not a Collider2D".into());
                }
                x.SawCollider = key;
                x.saw_collider_id = inst.objects[key.0].components[key.1];
                x.IdleLoop = audio_source_field(prefab, inst, file, v, "ChainsawBehaviour", "IdleLoop")?;
                x.RevLoop = audio_source_field(prefab, inst, file, v, "ChainsawBehaviour", "RevLoop")?;
                x.LayerMask = field(field(v, "LayerMask")?, "m_Bits")?.i64().ok_or("LayerMask.m_Bits")? as u32;
                x.Saw = self.sprite_field(&l, v, "Saw")?.unwrap_or_else(|| unimplemented!("mod-only: ChainsawBehaviour.Saw null (NullReferenceException in Start)"));
                // OnAfterDeserialise reconnects SerialisableVictims; a field the prefab omits is an empty array.
                if v.get("SerialisableVictims").and_then(Value::array).is_some_and(|a| !a.is_empty()) {
                    unimplemented!("mod-only: ChainsawBehaviour.SerialisableVictims: OnAfterDeserialise (GUID lookup of the victims)");
                }
                chainsaws.insert(o, self.chainsaws.len());
                self.chainsaws.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "EMPBehaviour") {
                let mut x = super::emp::Emp::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: EMPBehaviour on an object without a PhysicalBehaviour"));
                // Activate's Instantiate(Effect, ...) throws on a null Effect (ArgumentException).
                x.effect_null = field(v, "Effect")?.pptr().filter(|p| !p.is_null()).is_none();
                emps.insert(o, self.emps.len());
                self.emps.push(x);
            }
            if let Some((c, enabled, _)) = script(prefab, o, "FreezeBehaviour") {
                let mut x = super::freeze::Freeze::default();
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: FreezeBehaviour without a Rigidbody2D (NullReferenceException in Start)"));
                freezes.insert(o, self.freezes.len());
                self.freezes.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "FreezeSerialisationHelper") {
                let mut x = super::freeze::FreezeSerialisation::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                freeze_serialisations.insert(o, self.freeze_serialisations.len());
                self.freeze_serialisations.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "CeilingTurretBehaviour") {
                let mut x = super::ceiling_turret::CeilingTurret::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                // FirearmBehaviour: the firearm on its object.
                let fr = field(v, "FirearmBehaviour")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: CeilingTurretBehaviour.FirearmBehaviour null or outside its prefab (NullReferenceException in Update)"));
                let (fo, _) = *prefab.index.get(&ObjRef { file, path_id: fr.path_id }).ok_or("CeilingTurretBehaviour.FirearmBehaviour: dangling reference")?;
                x.firearm = *firearms.get(&fo).unwrap_or_else(|| unimplemented!("mod-only: CeilingTurretBehaviour.FirearmBehaviour on an object without a FirearmBehaviour in this clone"));
                // Joint: a HingeJoint2D of this prefab.
                let jt = field(v, "Joint")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: CeilingTurretBehaviour.Joint null or outside its prefab (NullReferenceException in Update)"));
                let at = *prefab.index.get(&ObjRef { file, path_id: jt.path_id }).ok_or("CeilingTurretBehaviour.Joint: dangling reference")?;
                x.joint = *l.arenas.hinge.get(&at).ok_or("CeilingTurretBehaviour.Joint: not a HingeJoint2D")?;
                x.LayerMask = field(field(v, "LayerMask")?, "m_Bits")?.i64().ok_or("LayerMask.m_Bits")? as u32;
                ceiling_turrets.insert(o, self.ceiling_turrets.len());
                self.ceiling_turrets.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "TemperatureTransferBehaviour") {
                let mut x = super::temperature_transfer::TemperatureTransfer::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                // TemperatureTransferBehaviour.Awake: GetComponent<PhysicalBehaviour>() on its object.
                match l.arenas.physical.get(&o) {
                    Some(&p) => x.phys = p,
                    None => x.phys_null = true,
                }
                // Other: the serialized PhysicalBehaviour (null throws in FixedUpdate).
                match l.physical(field(v, "Other")?)? {
                    Some(p) => x.Other = p,
                    None => x.other_null = true,
                }
                temperature_transfers.insert(o, self.temperature_transfers.len());
                self.temperature_transfers.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "LaserTurretBehaviour") {
                let mut x = super::laser_turret::LaserTurret::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                // physicalBehaviour: the turret's PhysicalBehaviour (null throws in FixedUpdate).
                x.physical = l.physical(field(v, "physicalBehaviour")?)?.unwrap_or_else(|| unimplemented!("mod-only: LaserTurretBehaviour.physicalBehaviour null (NullReferenceException in FixedUpdate)"));
                let jt = field(v, "joint")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: LaserTurretBehaviour.joint null or outside its prefab (NullReferenceException in FixedUpdate)"));
                let at = *prefab.index.get(&ObjRef { file, path_id: jt.path_id }).ok_or("LaserTurretBehaviour.joint: dangling reference")?;
                x.joint = *l.arenas.hinge.get(&at).ok_or("LaserTurretBehaviour.joint: not a HingeJoint2D")?;
                x.LayerMask = field(field(v, "layerMask")?, "m_Bits")?.i64().ok_or("LayerMask.m_Bits")? as u32;
                // projectile: the bolt prefab; BreakEffect and GlowEffect (SetAsBroken). The audio and flash are presentation.
                x.projectile = db.resolve(file, field(v, "projectile")?.pptr().ok_or("LaserTurretBehaviour.projectile")?);
                x.BreakEffect = db.resolve(file, field(v, "BreakEffect")?.pptr().ok_or("LaserTurretBehaviour.BreakEffect")?);
                x.GlowEffect = self.plain_field(&l, v, "GlowEffect")?;
                laser_turrets.insert(o, self.laser_turrets.len());
                self.laser_turrets.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "SentryTurretBehaviour") {
                let mut x = super::sentry_turret::SentryTurret::default();
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                // SentryTurretBehaviour.Awake: GetComponent<PhysicalBehaviour>(); State = Idle; MaxAngle = 100.
                x.phys = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: SentryTurretBehaviour without a PhysicalBehaviour (NullReferenceException in FixedUpdate)"));
                x.controller.MaxAngle = 100.0;
                x.BlockingLayers = field(field(v, "BlockingLayers")?, "m_Bits")?.i64().ok_or("BlockingLayers.m_Bits")? as u32;
                x.state = super::sentry_turret::SentryState::Idle;
                x.disabled = field(v, "disabled")?.bool().ok_or("SentryTurretBehaviour.disabled")?;
                x.timeStartedSearching = float(v, "timeStartedSearching")?;
                // turretGun: the SentryTurretGunBehaviour's object (its index after both are linked).
                let gun = field(v, "turretGun")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: SentryTurretBehaviour.turretGun null or outside its prefab (NullReferenceException in FixedUpdate)"));
                x.gun_object = prefab.index.get(&ObjRef { file, path_id: gun.path_id }).ok_or("SentryTurretBehaviour.turretGun: dangling reference")?.0;
                // AudioSource: its component id; the clip lists' lengths for PlaySound's draws and isPlaying.
                let src = field(v, "AudioSource")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: SentryTurretBehaviour.AudioSource null (NullReferenceException in PlaySound)"));
                let k = *prefab.index.get(&ObjRef { file, path_id: src.path_id }).ok_or("SentryTurretBehaviour.AudioSource: dangling reference")?;
                x.audio = inst.objects[k.0].components[k.1];
                x.clips = super::sentry_turret::SentryClips {
                    TargetAcquired: audio_clips(db, &mut self.audio.clip_cache, file, v, "TargetAcquired")?,
                    TargetLost: audio_clips(db, &mut self.audio.clip_cache, file, v, "TargetLost")?,
                    Flying: audio_clips(db, &mut self.audio.clip_cache, file, v, "Flying")?,
                    ShotAt: audio_clips(db, &mut self.audio.clip_cache, file, v, "ShotAt")?,
                    OnFire: audio_clips(db, &mut self.audio.clip_cache, file, v, "OnFire")?,
                    Tipped: audio_clips(db, &mut self.audio.clip_cache, file, v, "Tipped")?,
                    Retiring: audio_clips(db, &mut self.audio.clip_cache, file, v, "Retiring")?,
                };
                sentry_turrets.insert(o, self.sentry_turrets.len());
                self.sentry_turrets.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "SentryTurretGunBehaviour") {
                let mut x = super::sentry_turret::SentryTurretGun::default();
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                let jt = field(v, "turretJoint")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: SentryTurretGunBehaviour.turretJoint null or outside its prefab (NullReferenceException in FixedUpdate)"));
                let at = *prefab.index.get(&ObjRef { file, path_id: jt.path_id }).ok_or("SentryTurretGunBehaviour.turretJoint: dangling reference")?;
                x.joint = *l.arenas.hinge.get(&at).ok_or("SentryTurretGunBehaviour.turretJoint: not a HingeJoint2D")?;
                // firearmBehaviour: the FirearmBehaviour on this object (MinigunBehaviour's lookup).
                let fr = field(v, "firearmBehaviour")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: SentryTurretGunBehaviour.firearmBehaviour null or outside its prefab (NullReferenceException in FixedUpdate)"));
                let (fo, _) = *prefab.index.get(&ObjRef { file, path_id: fr.path_id }).ok_or("SentryTurretGunBehaviour.firearmBehaviour: dangling reference")?;
                x.firearm = *firearms.get(&fo).unwrap_or_else(|| unimplemented!("mod-only: SentryTurretGunBehaviour.firearmBehaviour on an object without a FirearmBehaviour in this clone"));
                // sentryBehaviour: the SentryTurretBehaviour's object (its index after both are linked).
                let sb = field(v, "sentryBehaviour")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: SentryTurretGunBehaviour.sentryBehaviour null or outside its prefab (NullReferenceException in FixedUpdate)"));
                x.sentry_object = prefab.index.get(&ObjRef { file, path_id: sb.path_id }).ok_or("SentryTurretGunBehaviour.sentryBehaviour: dangling reference")?.0;
                x.node = l.arenas.node[&o];
                x.line = l.add_line_on(self, db, o, "SentryTurretGunBehaviour")?;
                sentry_guns.insert(o, self.sentry_guns.len());
                self.sentry_guns.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "IonBoltBehaviour") {
                let mut x = super::ion_bolt::IonBolt::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.Layers = field(field(v, "Layers")?, "m_Bits")?.i64().ok_or("Layers.m_Bits")? as u32;
                // DeleteAfterTime: this object's (its Update loses a tenth of its life on each hit).
                let dat = field(v, "DeleteAfterTime")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: IonBoltBehaviour.DeleteAfterTime null (NullReferenceException in Update)"));
                let (dato, _) = *prefab.index.get(&ObjRef { file, path_id: dat.path_id }).ok_or("IonBoltBehaviour.DeleteAfterTime: dangling reference")?;
                x.delete_after = *delete_afters.get(&dato).ok_or("IonBoltBehaviour.DeleteAfterTime is not a DeleteAfterTime in this clone")?;
                // Awake: DeleteAfterTime.OnEndOfLife.AddListener (ion_bolt_end_of_life).
                self.delete_afters[x.delete_after].ion_bolt = Some(self.ion_bolts.len());
                for (name, slot) in [("ImpactEffect", &mut x.ImpactEffect), ("EndOfLifeEffect", &mut x.EndOfLifeEffect)] {
                    let p = field(v, name)?.pptr().ok_or(name)?;
                    if !p.is_null() {
                        let r = db.resolve(file, p).ok_or_else(|| format!("IonBoltBehaviour.{name} outside the data"))?;
                        if db.prefab(r)?.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                            unimplemented!("mod-only: IonBoltBehaviour: an {name} with physics (its pose)");
                        }
                        *slot = Some(r);
                    }
                }
                let decal = field(v, "ImpactDecal")?.pptr().ok_or("IonBoltBehaviour.ImpactDecal")?;
                if !decal.is_null() {
                    let r = db.resolve(file, decal).ok_or("IonBoltBehaviour.ImpactDecal outside the data")?;
                    x.ImpactDecal = Some(self.load_decal_descriptor(db, r)?);
                }
                let particles = l.object(field(v, "Particles")?)?.unwrap_or_else(|| unimplemented!("mod-only: IonBoltBehaviour.Particles is null (NullReferenceException at end of life)"));
                let physics = |x: usize| prefab.objects[x].components.iter().any(|(_, k)| matches!(k, Component::Rigidbody2D(_) | Component::Collider2D(_) | Component::Joint2D(_)));
                if physics(particles) || inst.descendants(particles).into_iter().any(physics) {
                    unimplemented!("mod-only: IonBoltBehaviour.Particles with physics (SetParent(null))");
                }
                x.Particles = inst.objects[particles].components[0];
                ion_bolts.insert(o, self.ion_bolts.len());
                self.ion_bolts.push(x);
            }
            // Every RandomEventBehaviour of the object (the Reactor's reactor_events has two).
            for (c, enabled, v) in scripts(prefab, o, "RandomEventBehaviour") {
                let mut x = super::random_event::RandomEvent::default();
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.ChancePerSecond = float(v, "ChancePerSecond")?;
                x.Cooldown = float(v, "Cooldown")?;
                for e in array(v, "Events")? {
                    // RandomEvent..ctor: Actions = new UnityEvent() and Delay 0, which the serializer then sets from the prefab entry.
                    let actions = e.get("Actions").ok_or("RandomEventBehaviour.Events.Actions")?;
                    x.events.push((float(e, "Delay")?, event_calls(prefab, file, actions, inst, "RandomEventBehaviour.Events", true)?));
                }
                random_events.insert(o, self.random_events.len());
                self.random_events.push(x);
            }
            for class in ["AtomBombBehaviour", "SeismicChargeBehaviour"] {
                if let Some((c, enabled, v)) = script(prefab, o, class) {
                    let mut x = super::prefab_bomb::PrefabBomb { id: inst.objects[o].components[c], enabled, atom: class == "AtomBombBehaviour", ..Default::default() };
                    x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: {class} without a PhysicalBehaviour on its GameObject"));
                    let e = field(v, "ExplosionPrefab")?.pptr().ok_or("ExplosionPrefab")?;
                    if !e.is_null() {
                        x.ExplosionPrefab = Some(db.resolve(file, e).ok_or_else(|| format!("{class}.ExplosionPrefab outside the data"))?);
                    }
                    prefab_bombs.insert(o, self.prefab_bombs.len());
                    self.prefab_bombs.push(x);
                }
            }
            if let Some((c, enabled, v)) = script(prefab, o, "NewAtomBombExplosionBehaviour") {
                let mut x = super::atom_explosion::AtomExplosion::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                if l.arenas.physical.contains_key(&o) || inst.objects[o].parent.is_some() {
                    unimplemented!("mod-only: NewAtomBombExplosionBehaviour on a moving or parented object (its transform.position)");
                }
                let t = inst.objects[o].trs.t;
                x.position = [t[0], t[1], t[2]];
                let local = |name: &str| -> Result<(usize, usize), String> {
                    let pp = field(v, name)?.pptr().ok_or_else(|| name.to_string())?;
                    prefab.index.get(&ObjRef { file, path_id: pp.path_id }).copied().filter(|_| !pp.is_null() && pp.file == 0).ok_or_else(|| format!("NewAtomBombExplosionBehaviour.{name} outside the prefab or null"))
                };
                x.ShockwaveCollider = local("ShockwaveCollider")?;
                if !matches!(&prefab.objects[x.ShockwaveCollider.0].components[x.ShockwaveCollider.1].1, Component::Collider2D(k) if matches!(k.shape, crate::assets::components::ColliderShape::Circle { .. })) {
                    return Err("NewAtomBombExplosionBehaviour.ShockwaveCollider is not a CircleCollider2D".into());
                }
                let (eo, ec) = local("PointEffector")?;
                if !matches!(prefab.objects[eo].components[ec].1, Component::PointEffector2D(_)) {
                    return Err("NewAtomBombExplosionBehaviour.PointEffector is not a PointEffector2D".into());
                }
                x.PointEffector = inst.objects[eo].components[ec];
                x.ShockwaveGrowth = Some(curve(field(v, "ShockwaveGrowth")?)?);
                x.ShockwaveSprite = self.sprite_field(&l, v, "ShockwaveSprite")?;
                atom_explosions.insert(o, self.atom_explosions.len());
                self.atom_explosions.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "WormStaffBehaviour") {
                let mut x = super::worm_staff::WormStaff::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.arenas.physical.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: WormStaffBehaviour without a PhysicalBehaviour"));
                if l.object(field(v, "Rigidbody")?)? != Some(o) {
                    unimplemented!("mod-only: WormStaffBehaviour.Rigidbody: not the staff's own Rigidbody2D");
                }
                x.Layer = field(field(v, "Layer")?, "m_Bits")?.i64().ok_or("WormStaffBehaviour.Layer.m_Bits")? as u32;
                // LineRenderer.transform: its object's local TRS and each parent's below the staff's own object.
                for r in field(v, "LineRenderers")?.array().ok_or("WormStaffBehaviour.LineRenderers")?.to_vec() {
                    let mut at = l.object(&r)?.unwrap_or_else(|| unimplemented!("mod-only: WormStaffBehaviour: a null LineRenderer (NullReferenceException in Start)"));
                    let mut chain = Vec::new();
                    while at != o {
                        chain.push(inst.objects[at].trs);
                        at = inst.objects[at].parent.ok_or("WormStaffBehaviour: a LineRenderer outside the staff")?;
                    }
                    x.LineRenderers.push(chain);
                    x.lines.push(l.add_line(self, db, &r, "WormStaffBehaviour")?);
                }
                x.VertexCount = field(v, "VertexCount")?.i64().ok_or("WormStaffBehaviour.VertexCount")? as i32;
                worm_staffs.insert(o, self.worm_staffs.len());
                self.worm_staffs.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "EnergySwordBehaviour") {
                let mut x = super::energy_sword::EnergySword::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.enabled = enabled;
                x.physical = l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.unwrap_or_else(|| unimplemented!("mod-only: EnergySwordBehaviour.PhysicalBehaviour null (NullReferenceException)"));
                let collider = |name: &str| -> Result<(usize, usize), String> {
                    let p = field(v, name)?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: EnergySwordBehaviour.{name} null or outside its prefab (NullReferenceException)"));
                    let k = *prefab.index.get(&ObjRef { file, path_id: p.path_id }).ok_or_else(|| format!("EnergySwordBehaviour.{name}: dangling reference"))?;
                    match prefab.objects[k.0].components[k.1].1 {
                        Component::Collider2D(_) => Ok(k),
                        _ => Err(format!("EnergySwordBehaviour.{name} is not a Collider2D")),
                    }
                };
                let main = collider("MainCollider")?;
                x.MainCollider = inst.objects[main.0].components[main.1];
                x.BladeCollider = collider("BladeCollider")?;
                x.blade_collider_id = inst.objects[x.BladeCollider.0].components[x.BladeCollider.1];
                let line = field(v, "LocalBladeLine")?;
                for (k, name) in ["A", "B"].into_iter().enumerate() {
                    super::Field::set(&mut x.LocalBladeLine[k], field(line, name)?).map_err(|e| format!("EnergySwordBehaviour.LocalBladeLine.{name}: {e}"))?;
                }
                x.BladeObject = l.object(field(v, "BladeObject")?)?.unwrap_or_else(|| unimplemented!("mod-only: EnergySwordBehaviour.BladeObject null (NullReferenceException in Awake)"));
                x.MotionBlur = l.object(field(v, "MotionBlur")?)?.unwrap_or_else(|| unimplemented!("mod-only: EnergySwordBehaviour.MotionBlur null (NullReferenceException in Awake)"));
                x.LayerMask = field(field(v, "LayerMask")?, "m_Bits")?.i64().ok_or("LayerMask.m_Bits")? as u32;
                x.ImpactSound = array(v, "ImpactSound")?.len();
                x.LoopSource = audio_source_field(prefab, inst, file, v, "EnergySwordBehaviour", "LoopSource")?;
                // SetActivation plays and stops these and the particles unconditionally.
                for name in ["HitParticle", "BoilingVFX", "RainParticles", "LoopSource"] {
                    if l.object(field(v, name)?)?.is_none() {
                        unimplemented!("mod-only: EnergySwordBehaviour.{name} null (NullReferenceException in SetActivation)");
                    }
                }
                // ImpactSource: its isPlaying draws Random.value on Play. A looping, enabled source with a clip plays
                // until Stop.
                let a = field(v, "ImpactSource")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: EnergySwordBehaviour.ImpactSource null or outside its prefab"));
                let source = crate::assets::classes::AudioSource::read(db.data(ObjRef { file, path_id: a.path_id }))?;
                let so = prefab.index[&ObjRef { file, path_id: a.path_id }].0;
                if !source.enabled || source.clip.is_null() || !source.looping || so == x.BladeObject || inst.descendants(x.BladeObject).contains(&so) {
                    unimplemented!("mod-only: EnergySwordBehaviour.ImpactSource: disabled, without a clip, not looping or under BladeObject (its isPlaying)");
                }
                if v.get("SerialisableVictims").and_then(Value::array).is_some_and(|a| !a.is_empty()) {
                    unimplemented!("mod-only: EnergySwordBehaviour.SerialisableVictims: OnAfterDeserialise (GUID lookup of the victims)");
                }
                energy_swords.insert(o, self.energy_swords.len());
                self.energy_swords.push(x);
            }
            if let Some((c, enabled, v)) = script(prefab, o, "EnergySwordMotionBlur") {
                if !enabled {
                    unimplemented!("mod-only: a disabled EnergySwordMotionBlur");
                }
                let mut x = super::energy_sword::EnergySwordBlur::default();
                load_values(x.fields(), v)?;
                x.id = inst.objects[o].components[c];
                x.Life = float(v, "Life")?;
                x.MinDistance = float(v, "MinDistance")?;
                x.ColorOverMesh = super::visual::Gradient::read(field(v, "ColorOverMesh")?)?;
                for (name, end) in [("A", &mut x.A), ("B", &mut x.B)] {
                    let t = l.object(field(v, name)?)?.unwrap_or_else(|| unimplemented!("mod-only: EnergySwordMotionBlur.{name} null (NullReferenceException in Update)"));
                    *end = l.anchor_chain(t);
                }
                let mp = field(v, "MeshRenderer")?.pptr().filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| unimplemented!("mod-only: EnergySwordMotionBlur.MeshRenderer null or outside its prefab (NullReferenceException in Awake)"));
                let (mo, mc) = *prefab.index.get(&ObjRef { file, path_id: mp.path_id }).ok_or("EnergySwordMotionBlur.MeshRenderer: dangling reference")?;
                if !matches!(prefab.objects[mo].components[mc].1, Component::Other(crate::assets::components::MeshRenderer::CLASS)) {
                    return Err("EnergySwordMotionBlur.MeshRenderer is no MeshRenderer".into());
                }
                let r = crate::assets::components::MeshRenderer::read(db.data(prefab.objects[mo].components[mc].0))?;
                if r.materials.len() != 1 {
                    unimplemented!("mod-only: EnergySwordMotionBlur.MeshRenderer with {} materials", r.materials.len());
                }
                x.renderer_id = inst.objects[mo].components[mc];
                x.renderer_enabled = r.enabled;
                x.material = if r.materials[0].is_null() { None } else { db.resolve(file, r.materials[0]) };
                // Awake: MeshRenderer.sortingLayerName = "Bottom" with SendToBottomLayer.
                x.sorting_layer = if x.SendToBottomLayer { crate::play::SORTING_LAYERS[0] as i32 } else { r.sorting_layer_id };
                x.sorting_order = r.sorting_order;
                x.game_layer = prefab.objects[mo].layer as i32;
                x.visible = inst.objects[mo].active;
                self.energy_blurs.push(x);
            }
            if let Some((c, _, v)) = script(prefab, o, "EnergySwordBladeClashBehaviour") {
                let p = field(v, "CollideWithOtherSwordVFX")?.pptr().ok_or("CollideWithOtherSwordVFX")?;
                let r = if p.is_null() { ObjRef { file: 0, path_id: 0 } } else { db.resolve(file, p).ok_or("EnergySwordBladeClashBehaviour.CollideWithOtherSwordVFX outside the data")? };
                if !p.is_null() && db.prefab(r)?.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_)))) {
                    unimplemented!("mod-only: EnergySwordBladeClashBehaviour: a CollideWithOtherSwordVFX with physics (its pose)");
                }
                let colliders = prefab.objects[o].components.iter().enumerate().filter(|(_, (_, k))| matches!(k, Component::Collider2D(_))).map(|(k, _)| inst.objects[o].components[k]).collect();
                self.blade_clashes.push((inst.objects[o].components[c], colliders, r));
            }
            if let Some((_, _, v)) = script(prefab, o, "SignalConverterBehaviour") {
                let mut x = super::signal_converter::SignalConverter::default();
                load_values(x.fields(), v)?;
                x.wired = script(prefab, o, "UseWireBehaviour").is_some();
                x.physical = l.arenas.physical.get(&o).copied();
                signal_converters.insert(o, self.signal_converters.len());
                self.signal_converters.push(x);
            }
        }
        // SentryTurretBehaviour.turretGun and SentryTurretGunBehaviour.sentryBehaviour: each other's index, once both exist.
        for &i in sentry_turrets.values() {
            let go = self.sentry_turrets[i].gun_object;
            self.sentry_turrets[i].gun = *sentry_guns.get(&go).ok_or("SentryTurretBehaviour.turretGun is not a SentryTurretGunBehaviour")?;
        }
        for &i in sentry_guns.values() {
            let go = self.sentry_guns[i].sentry_object;
            self.sentry_guns[i].sentry = *sentry_turrets.get(&go).ok_or("SentryTurretGunBehaviour.sentryBehaviour is not a SentryTurretBehaviour")?;
        }
        for (j, lp) in jms_links {
            if let Some(o) = lp {
                self.joint_motor_sounds[j].loop_sound = Some(*loop_sounds.get(&o).ok_or("JointMotorSoundBehaviour.LoopSoundBehaviour: not a LoopSoundBehaviour")?);
            }
        }
        for (display, core, wires) in a5_links {
            self.reactor_a5_displays[display].Core = core.map(|o| reactor_cores.get(&o).copied().ok_or("ReactorA5DisplayBehaviour.Core: not a ReactorCoreBehaviour")).transpose()?;
            for (k, w) in wires.into_iter().enumerate() {
                self.reactor_a5_displays[display].Wires[k] = w.map(|o| reactor_conduit_sets.get(&o).copied().ok_or("ReactorA5DisplayBehaviour.Wire: not a ReactorConduitSetBehaviour")).transpose()?;
            }
        }
        for (core, sets, sequence) in reactor_links {
            for o in sets {
                self.reactor_cores[core].conduits.push(*reactor_conduit_sets.get(&o).ok_or("ReactorCoreBehaviour.EnergyConduits: not a ReactorConduitSetBehaviour")?);
            }
            if let Some(o) = sequence {
                self.reactor_cores[core].sequence = Some(*do_at_times.get(&o).ok_or("ReactorCoreBehaviour.MeltdownSequence: not a DoAtTime")?);
            }
        }
        for (conduit, target) in conduit_links {
            if let Some(o) = target {
                self.reactor_conduits[conduit].LiquidPushTarget = Some(*reactor_conduits.get(&o).ok_or("ReactorConduitBehaviour.LiquidPushTarget: not a ReactorConduitBehaviour")?);
            }
        }
        // Every other ParticleSystem of the clone (debris dust, a vase's SmallDebris): its Awake plays it when
        // playOnAwake. A sub-emitter is its parent system's to play. An effect clone that does not move is the
        // effect's (`effect_log` plays its systems).
        let moves = prefab.objects.iter().any(|o| o.components.iter().any(|(_, c)| matches!(c, Component::Rigidbody2D(_))));
        let mut subs = std::collections::HashSet::new();
        for ob in &prefab.objects {
            for (r, c) in &ob.components {
                if matches!(c, Component::Other(super::water::PARTICLE_SYSTEM)) {
                    let ps = crate::assets::particles::ParticleSystem::read(db.data(*r)).map_err(|e| format!("ParticleSystem: {e}"))?;
                    subs.extend(ps.subs.iter().filter_map(|s| db.resolve(file, s.emitter)));
                }
            }
        }
        let mut active = Vec::with_capacity(n);
        for o in 0..if moves || !self.effect_spawn { n } else { 0 } {
            active.push(prefab.objects[o].active && inst.objects[o].parent.is_none_or(|p| active[p]));
            let go = prefab.objects[o].game_object;
            for (k, (r, c)) in prefab.objects[o].components.iter().enumerate() {
                if !matches!(c, Component::Other(super::water::PARTICLE_SYSTEM)) || subs.contains(r) || self.embedded_ps[embedded_from..].iter().any(|e| e.go == go) {
                    continue;
                }
                self.add_embedded_ps_at(&l, prefab, inst, o, inst.objects[o].components[k], super::EmbeddedDrive::Free);
                // On an inactive object: its Awake (and playOnAwake's Play) comes with the activation.
                self.embedded_ps.last_mut().unwrap().awake = active[o];
            }
        }
        let linked = Linked { control_replacers, visuals, inverters, obstruction_emitters, person, limbs: l.arenas.limb, nodes: l.arenas.node, physicals: l.arenas.physical, hinges: l.arenas.hinge, conveyors, reactor_cores, reactor_conduits, reactor_conduit_sets, reactor_meltdowns, do_at_times, industrial_lifts, copper_wires, eps, acid_pools, vacuums, point_ats, random_sounds, reactor_a5_displays, loop_sounds, joint_motor_sounds, map_lights, waters, machinery, destroyables, fire_maps, firearms, launchers, bells, machine_guns, miniguns, accelerators, accelerator_bolts, archelix_casters, archelix_bolts, universal_bells, counters, pointers, toggleable_mirrors, valves, liquid_canisters, liquid_filters, pulse_drums, liquidentifiers, centrifuges, vibrations, liquid_pumps, liquid_duplicators, liquid_holes, timed_gates, lagboxes, gates, activation_transformers, hover_thrusters, hovercars, winches, jukeboxes, antenna_receivers, couplers, lightning_entities, pyro_entities, seismic_explosions, energy_bolts, transmitters, defibrillators, cardiopulmonary_bypasses, heart_monitors, blood_tanks, liquid_outlets, excaliburs, sharp_on_all_sides, explosives, delete_afters, point_to_velocities, android_sparks, cars, act_on_collides, tyres, flasks, truck_doors, liquid_containers, glowtubes, cathode_lights, leds, detectors, sticky_grenades, liquid_level_gates, generators, floodlights, spike_grenades, heating_elements, ray_guns, bulbs, thrusterbeds, pistons, map_lifts, flamethrowers, rockets, airfoils, stunners, soaps, power_hammers, balloons, metronomes, physics_guns, activator_electrodes, thermometers, infrared_thermometers, activation_toggles, power_gauges, torches, flares, buttons, displays, lasers, firearm_attachments, televisions, syringes, jet_engines, chainsaws, freezes, emps, freeze_serialisations, ceiling_turrets, temperature_transfers, worm_staffs, atom_explosions, prefab_bombs, laser_turrets, sentry_turrets, sentry_guns, ion_bolts, random_events,decimators, drones, portals, black_holes, immobility_fields, lightning_guns, javelins, javelin_missiles, tesla_coils, tesla_bolt_sources, energy_swords, blasters, beam_cannons, beam_repeaters, android_lasers, ray_cannons, railguns, laser_receivers, key_triggers, fireworks, blasterbolts, beamformers, scifi40s, effect_guns, signal_converters, syringe_explosions, point_liquids, lavas, spawners, heat_rects, random_sprites, node_rigidbodies, parent_audio: None, translated, determined_anomalies, random_sprite_children, play_random_clips };
        self.register_script_slots(prefab, inst, &linked);
        Ok(linked)
    }

    /// PP_STOP_PROBE=Class,Class (tools only): spawns the root of every shipped use of each class and prints the panic of
    /// those that stop (which stop the real data reaches).
    pub fn stop_probe(&mut self, classes: &[String]) {
        let mut db = self.assets.take().expect("stop_probe needs the game's data");
        let last = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let l2 = last.clone();
        std::panic::set_hook(Box::new(move |i| *l2.lock().unwrap() = format!("{} @ {}", i.payload().downcast_ref::<String>().cloned().or(i.payload().downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default(), i.location().map_or(String::new(), |l| format!("{}:{}", l.file(), l.line())))));
        for class in classes {
            if class == "@catalog" {
                for name in std::fs::read_to_string("tools/pc/items.txt").unwrap().lines().skip(std::env::var("PP_CAT_FROM").ok().and_then(|x| str::parse::<usize>(&x).ok()).unwrap_or(0)).map(str::trim).filter(|x| !x.is_empty()) {
                    println!("CATNOW {name}");
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let prefab = Self::get_spawnable(&mut db, name)?;
                        let (_inst, linked) = self.spawn(&mut db, &prefab, [0.0; 3], [0.0, 0.0, 0.0, 1.0], None)?;
                        self.spawnable_asset_components(&prefab, &linked, name, 0);
                        Ok::<(), String>(())
                    }));
                    match r { Ok(Ok(())) => {}, Ok(Err(e)) => println!("CAT {name}: Err {e}"), Err(_) => println!("CAT {name}: PANIC {}", last.lock().unwrap()) }
                }
                continue;
            }
            let Ok(mbs) = db.find_scripts(class) else { println!("PROBE {class}: find_scripts failed"); continue };
            let (mut n, mut seen) = (0, std::collections::HashSet::new());
            for mb in mbs {
                n += 1;
                let go = db.resolve(mb.file, crate::assets::classes::MonoBehaviour::read(db.data(mb)).unwrap().game_object).unwrap();
                let mut root = go;
                loop {
                    let g = db.game_object(root).unwrap();
                    let Some(t) = g.components.iter().filter_map(|p| db.resolve(root.file, *p)).find(|r| db.class(*r) == 4) else { break };
                    let tr = crate::assets::classes::Transform::read(db.data(t)).unwrap();
                    let Some(f) = db.resolve(t.file, tr.father) else { break };
                    root = db.resolve(f.file, crate::assets::classes::Transform::read(db.data(f)).unwrap().game_object).unwrap();
                }
                if !seen.insert(root) {
                    continue;
                }
                let name = db.game_object(root).map(|g| g.name).unwrap_or_default();
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let prefab = db.prefab(root)?;
                    let (mut inst, _) = self.spawn(&mut db, &prefab, [0.0; 3], [0.0, 0.0, 0.0, 1.0], None)?;
                    // Objects that start inactive wake their scripts at their first SetActive(true).
                    for o in 0..inst.objects.len() {
                        if !inst.objects[o].active {
                            self.set_active_scripts(&mut inst, o, true);
                        }
                    }
                    Ok::<(), String>(())
                }));
                match r {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => println!("PROBE {class} root {name:?}: Err {e}"),
                    Err(_) => println!("PROBE {class} root {name:?}: PANIC {}", last.lock().unwrap()),
                }
            }
            println!("PROBE {class}: {n} uses, {} roots", seen.len());
        }
        self.assets = Some(db);
    }

    /// Object.Instantiate of a Person prefab up to the end of the Instantiate call: the native clone and its
    /// AwakeFromLoad queues, the scripts' deserialization (`link_instance`), then queue 0x14's Awake calls.
    /// The Starts are queued: Unity runs them in the spawn frame, PersonBehaviour's before its LateUpdate and the
    /// rest after the LateUpdate phase (`update_scripts`). The b2 bodies see PersonBehaviour.Awake's rescale at the
    /// next SyncTransforms.
    /// `parent_audio` is the PlayAudioAtRandom of the parent the clone goes under (`ambient_audio`).
    pub fn spawn(&mut self, db: &mut AssetDb, prefab: &Prefab, position: [f32; 3], rotation: [f32; 4], parent_audio: Option<usize>) -> Result<(Instance, Linked), String> {
        let tt = std::time::Instant::now();
        let tp = |w: &str| if std::env::var_os("PP_SPAWN_BENCH").is_some() { eprintln!("BENCH     {w} {:.2} ms", tt.elapsed().as_secs_f64() * 1000.0) };
        let original_ids = db.load_order()?;
        tp("order");
        // A replay: Instantiate's ids continue from every object the game made since the recording began, so the clone
        // takes the ids that put its first PhysicalBehaviour on the next one the recording lists (pre-order, 2 apart).
        let slot = prefab.objects.iter().flat_map(|o| std::iter::once(false).chain(o.components.iter().map(|(_, c)| matches!(c, Component::Script { class, .. } if class == "PhysicalBehaviour")))).position(|pb| pb);
        if let Some(k) = slot {
            if let Some(id) = self.recorded_hashes.next_spawn() {
                self.next_instance_id = id + 2 * k as i32;
            }
        }
        let (mut inst, behaviours) = self.instantiate_native(prefab, position, rotation, &original_ids);
        tp("native");
        let first = self.physicals.len();
        let mut linked = self.link_instance(db, prefab, &inst)?;
        tp("link");
        linked.parent_audio = parent_audio;
        // GetHashCode (Random.InitState in PhysicalBehaviour.Start) is Mono's object hash, (address >> 3) *
        // 0x9e3779b1 (mono_object_hash, 180116520), of the managed heap address. That address is not defined by the
        // game: it varies from run to run with the OS's heap placement. The rewrite must not (the same input gives the
        // same frames), so the address is modelled: Boehm hands out large objects upward from the heap base, so the
        // n-th PhysicalBehaviour sits `n` objects above it. A PhysicalBehaviour is a large object, 2048-aligned (every
        // recorded hash has a zero low byte). Recorded runs (dump.rs) carry the real hash instead.
        for p in first..self.physicals.len() {
            let address = self.heap_base.unwrap_or(MODELLED_HEAP_BASE) + ((p as u64) << 11);
            let modelled = ((address >> 3) as u32).wrapping_mul(0x9e37_79b1) as i32;
            let id = self.physicals[p].id;
            let active = inst.objects.iter().find(|o| o.components.contains(&id)).map_or(true, |o| o.active);
            self.physicals[p].hash_code = self.recorded_hashes.take(id, active).unwrap_or(modelled);
        }
        tp("hash");
        self.awake_behaviours(Some(&mut *db), prefab, &mut inst, &linked, &behaviours);
        tp("awake");
        self.queue_starts(Some(&mut *db), prefab, &inst, &linked, &behaviours)?;
        tp("starts");
        if inst.objects.iter().any(|x| !x.activated) {
            self.first_activation.insert(inst.objects[0].components[0], linked.clone());
        }
        // extras: a portal gun (src/extras_hook/portal_gun.rs), spawned, loaded or pasted, joins the guns.
        self.extras_spawned(prefab, &linked);
        Ok((inst, linked))
    }

    /// HologramJitter.Awake: offset = Random.Range(-1f, 1f) * 1500f.
    pub(crate) fn hologram_jitter_offset(&mut self) -> f32 {
        (super::d(self.random.range(-1.0, 1.0)) * 1500.0f32).f()
    }

    /// ColorWave.OnEnable: oV = Random.value, sV = Random.value.
    pub(super) fn color_wave_on_enable(&mut self) {
        self.random.value();
        self.random.value();
    }

    /// ResetTransformBehaviour.ResetTransform
    /// The linked objects' Transforms after a script changed the clone's.
    /// DetachFromParentOnStart.Start on the clone object holding component `id`: with a parent,
    /// DestroyIfParentDestroyed adds DestroyThis to the parent's ActOnDestroy (GetOrAddComponent), then
    /// Transform.SetParent(null).
    /// ResetTransformBehaviour.ResetTransform. TransformReset.Position is 0, so HasFlag(Position) is always true.
    pub(crate) fn reset_transform(&mut self, id: i32, to_reset: i64) {
        self.with_component_instance(id, |_, inst, o| {
            inst.set_local_position(o, [0.0; 3]);
            if to_reset & 1 != 0 {
                inst.set_local_rotation(o, [0.0, 0.0, 0.0, 1.0]);
            }
            if to_reset & 2 != 0 {
                inst.set_local_scale(o, [1.0; 3]);
            }
        });
    }

    /// DetachFromParentOnStart.Start (DestroyThis is the DestroyThis listener).
    pub(crate) fn detach_from_parent_on_start(&mut self, id: i32, destroy_with_parent: bool) {
        // A recorded item's: detached in the recording already; the listener on its former parent only.
        if let Some(parent) = self.recorded_detached.remove(&id) {
            if destroy_with_parent {
                self.with_component_instance(id, |g, inst, _| {
                    let key = inst.objects[parent].components[0];
                    if !g.act_on_destroy.contains_key(&key) {
                        g.act_on_destroy_add(inst, parent);
                    }
                    g.act_on_destroy.entry(key).or_default().listeners.push(super::destroyable::Listener::DestroyThis(id));
                });
            }
            return;
        }
        self.set_parent_null_of(id, |g, inst, parent| {
            if destroy_with_parent {
                // GetOrAddComponent<ActOnDestroy>
                let key = inst.objects[parent].components[0];
                if !g.act_on_destroy.contains_key(&key) {
                    g.act_on_destroy_add(inst, parent);
                }
                g.act_on_destroy.entry(key).or_default().listeners.push(super::destroyable::Listener::DestroyThis(id));
            }
        });
    }

    /// A persistent call `event_calls` kept.
    pub(super) fn event_call(&mut self, call: EventCall) {
        match call {
            EventCall::Audio(id, a) => self.audio_call(id, a),
            EventCall::Car(car, c) => self.car_invoke(car, c),
            EventCall::LiquidSpread(id) if self.destroyed_components.contains(&id) => {}
            EventCall::LiquidSpread(id) => self.liquid_spread(id),
            EventCall::FlamethrowerAoe(id) if self.destroyed_components.contains(&id) => {}
            EventCall::FlamethrowerAoe(id) => self.flamethrower_affect_aoe(id),
            EventCall::MapLiftSetFloor(id, _) | EventCall::MapLiftEmergencyStop(id) if self.destroyed_components.contains(&id) => {}
            EventCall::MapLiftSetFloor(id, floor) => self.map_lift_set_floor(id, floor),
            EventCall::MapLiftEmergencyStop(id) => self.map_lift_emergency_stop(id),
            // DetachFromParent.Detach: transform.parent = null.
            EventCall::Detach(id) => self.set_parent_null_of(id, |_, _, _| {}),
            // ActOnDestroy.Clear: DestroyWith = null. A destroyed ActOnDestroy (its entry is gone) is a destroyed target: the call is skipped.
            EventCall::ClearDestroyWith(key) => {
                if let Some(a) = self.act_on_destroy.get_mut(&key) {
                    a.destroy_with.clear();
                }
            }
            // A destroyed target's call is skipped (BaseInvokableCall.AllowInvoke).
            EventCall::EnableDeleteAfter(id) => self.enable_delete_after(id),
            // Behaviour.enabled = true on a RandomForceAtStart that never ran Awake: Start follows before its first
            // Update (it destroys itself, so it only enables once).
            EventCall::EnableRandomForce(rb, ..) if self.destroyed_components.contains(&rb) => {}
            EventCall::EnableRandomForce(rb, positional, rotational) => self.pending_starts.push(super::misc::PendingStart::RandomForce(rb, positional, rotational)),
            EventCall::Simulate(id) if self.destroyed_components.contains(&id) => {}
            EventCall::Simulate(id) => self.rigidbody_set_simulated(id),
            EventCall::Instantiate(id) if self.destroyed_components.contains(&id) => {}
            EventCall::Instantiate(id) => self.instantiate_object(id, false),
            EventCall::AddRelativeForce(id, _) if self.destroyed_components.contains(&id) => {}
            EventCall::AddRelativeForce(id, intensity) => self.add_force_relative(id, intensity),
            EventCall::HaltNextPropagation(id) if self.destroyed_components.contains(&id) => {}
            EventCall::HaltNextPropagation(id) => self.physical_halt_next_propagation(id),
            EventCall::SetBreakForce(id, _) | EventCall::WakeUp(id) | EventCall::AddTorque(id, _) if self.destroyed_components.contains(&id) => {}
            EventCall::SetBreakForce(id, v) => self.joint_set_break_force(id, v),
            EventCall::WakeUp(id) => self.rigidbody_wake_up(id),
            EventCall::AddTorque(id, torque) => self.rigidbody_add_torque(id, torque),
            // Behaviour.enabled of a UseEventTrigger: Use reads it; it has no OnEnable or OnDisable.
            EventCall::DisableMachinery(id) => {
                let m = self.machinery.iter().position(|m| m.id == id).expect("a DamagableMachineryBehaviour in the arena");
                self.behaviours.remove(Behaviour::Machinery(m));
            }
            EventCall::MachineryBreakPermanently(id) if self.destroyed_components.contains(&id) => {}
            EventCall::MachineryBreakPermanently(id) => {
                let m = self.machinery.iter().position(|m| m.id == id).expect("a DamagableMachineryBehaviour in the arena");
                self.machinery_break_permanently(m);
            }
            EventCall::JetForceShutoff(id) if self.destroyed_components.contains(&id) => {}
            EventCall::JetForceShutoff(id) => self.jet_engine_force_shutoff(id),
            // InvokableCall skips a destroyed target.
            EventCall::LaserUpdateActivation(id) if self.destroyed_components.contains(&id) => {}
            EventCall::LaserUpdateActivation(id) => {
                let l = self.lasers.iter().position(|x| x.id == id).expect("a LaserBehaviour in the arena");
                self.laser_update_activation(l);
            }
            EventCall::DisableTankBarrel(id) | EventCall::DisableJoint(id) if self.destroyed_components.contains(&id) => {}
            EventCall::SetActive(id, _) if self.destroyed_components.contains(&id) => {}
            EventCall::SetActive(id, on) => self.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, on)),
            EventCall::DisableTankBarrel(id) => {
                let Some(&(_, Behaviour::Translated(super::translated::TBehaviour::TankBarrelBehaviour(i)))) = self.behaviour_of.get(&id) else { unreachable!() };
                self.tr.tank_barrel_behaviour[i].enabled = false;
            }
            EventCall::DisableJoint(id) => self.with_component_instance(id, |g, inst, o| {
                let c = inst.objects[o].components.iter().position(|&x| x == id).unwrap();
                let prefab = inst.prefab.clone();
                g.joint2d_deactivate(inst, &prefab, o, c);
            }),
            EventCall::ArcFlash(id) | EventCall::ArcSmallArc(id) | EventCall::RopeCreate(id) if self.destroyed_components.contains(&id) => {}
            EventCall::RopeCreate(id) => self.rope_create(id),
            EventCall::ArcFlash(id) => self.reactor_a5_arc_flash(id),
            EventCall::ArcSmallArc(id) => self.reactor_a5_arc_generate_small_arc(id),
            EventCall::ReactorCore(id, _) | EventCall::MeltdownExplode(id) if self.destroyed_components.contains(&id) => {}
            EventCall::ReactorCore(id, call) => self.reactor_core_call(id, call),
            EventCall::MeltdownExplode(id) => self.reactor_meltdown_explode(id),
            // StartCoroutine on a destroyed component is a MissingReferenceException-free no-op in InvokableCall.
            EventCall::StickyBlink(id) if self.destroyed_components.contains(&id) => {}
            EventCall::StickyBlink(id) => {
                let s = self.sticky_grenades.iter().position(|s| s.id == id).expect("a StickyGrenadeBehaviour in the arena");
                self.sticky_start_blinking(s);
            }
            EventCall::MachineryForceBreak(id) | EventCall::BreakJointSlice(id) | EventCall::IndustrialLiftSetSpeed(id, _) | EventCall::RandomSoundPlay(id) | EventCall::PlayRandomClipPlay(id) | EventCall::SetEnabled(_, id, _) if self.destroyed_components.contains(&id) => {}
            EventCall::MachineryForceBreak(id) => {
                let m = self.machinery.iter().position(|m| m.id == id).expect("a DamagableMachineryBehaviour in the arena");
                self.machinery_force_break(m);
            }
            EventCall::BreakJointSlice(id) => self.break_joint_on_shot_slice(id),
            EventCall::IndustrialLiftSetSpeed(id, v) => self.industrial_lift_set_speed(id, v),
            EventCall::RandomSoundPlay(id) => self.random_sound_play_clip(id),
            EventCall::PlayRandomClipPlay(id) => self.play_random_clip_play(id),
            EventCall::SetEnabled(class, id, true) => self.behaviour_enable(class, id),
            EventCall::SetEnabled(class, id, false) => self.behaviour_disable(class, id),
            EventCall::TargetLineRendererEnable(id, _) if self.destroyed_components.contains(&id) => {}
            EventCall::TargetLineRendererEnable(id, on) => {
                let Some(&(_, Behaviour::Translated(super::translated::TBehaviour::TargetLineRenderer(i)))) = self.behaviour_of.get(&id) else { unreachable!() };
                self.tr.target_line_renderer[i].enabled = on;
            }
            EventCall::AnimationPlay(id) => self.trivial_animation_play_id(id),
            EventCall::ExplosionSoundPlay(id, enabled, ..) if self.destroyed_components.contains(&id) || !self.explosion_sound_enabled(id, enabled) => {}
            EventCall::ExplosionSoundPlay(id, _, near, distant, delay) => self.explosion_sound_play(id, near, distant, delay),
            EventCall::ExplosionSoundEnable(id, ..) if self.destroyed_components.contains(&id) => {}
            EventCall::ExplosionSoundEnable(id, enabled, start) => self.explosion_sound_enable(id, enabled, start),
            EventCall::Stop(what) => unimplemented!("mod-only: {what}"),
            EventCall::SpriteEnabled(id, ..) if self.destroyed_components.contains(&id) => {}
            // Renderer.enabled = on: the physical's sprite when its object has a PhysicalBehaviour (`sprite_at`).
            EventCall::SpriteEnabled(id, physical, on) => {
                let s = match physical {
                    Some(p) => super::visual::SpriteAt::Physical(self.physicals.iter().position(|x| x.id == p).expect("a PhysicalBehaviour in the arena")),
                    None => super::visual::SpriteAt::Renderer(self.renderers.iter().position(|r| r.id == id).expect("a SpriteRenderer with a sprite")),
                };
                self.set_sprite_enabled(s, on);
            }
            EventCall::SetUseEventEnabled(id, _) if self.destroyed_components.contains(&id) => {}
            EventCall::SetUseEventEnabled(id, on) => self.use_events.get_mut(&id).expect("a UseEventTrigger in the arena").enabled = on,
        }
    }

    /// UseEventTrigger.Use (a Messages.IUse receiver): the Event's persistent calls in order, once with OnlyOnce.
    pub(crate) fn use_event_trigger_use(&mut self, id: i32) {
        let e = self.use_events.get_mut(&id).expect("a UseEventTrigger in the arena");
        if (e.only_once && e.use_count > 0) || !e.enabled {
            return;
        }
        e.use_count += 1;
        for call in e.calls.clone() {
            self.event_call(call);
        }
    }

    /// Transform.SetParent(null) of the clone object holding component `id`, when it has a parent; `before` runs
    /// first with the parent.
    pub(super) fn set_parent_null_of(&mut self, id: i32, before: impl FnOnce(&mut Game, &mut Instance, usize)) {
        let moved = self.with_component_instance(id, |g, inst, o| {
            let Some(parent) = inst.objects[o].parent else { return false };
            before(g, inst, parent);
            g.set_parent_null(inst, o);
            true
        });
        // Its changed Transforms reach the bodies at the next SyncTransforms.
        if moved && let Some(i) = self.retained.iter().position(|i| i.objects.iter().any(|x| x.components.contains(&id))) {
            let inst = self.retained.remove(i);
            self.unsynced.push(inst);
        }
    }

    /// NoChildrenCollide.Start on the clone object holding component `id`: RequestIgnoreCollision for every pair of
    /// GetComponentsInChildren<Collider2D>() (active objects, depth first, component order) on different Transforms.
    pub(crate) fn no_children_collide_start(&mut self, id: i32) {
        let colliders: Vec<(i32, usize)> = self.with_component_instance(id, |_, inst, o| colliders_in_children(inst, o));
        for &(a, ta) in &colliders {
            for &(b, tb) in &colliders {
                if a != b && ta != tb {
                    self.ignore_collision_substitute(a, b, true);
                }
            }
        }
    }

    /// Transform.SetParent(null) on clone object `o` (Instance::set_parent_null), and the TransformHierarchyChange
    /// systems for it and its children: PhysicsManager2D::HandleRigidbodyParentHierarchyChanges (180bf0f20) runs
    /// CheckForDrivenByParentRigidbody, ResetRigidbodyUpdates (back into the scene's depth lists, which sets the
    /// rigidbody-hierarchy-changed flag) and clears the movement state's interpolating byte (+0x114) of each
    /// Rigidbody2D; Collider2D::ParentChanged (180c124e0) re-attaches a collider whose FindRigidbody changed.
    fn set_parent_null(&mut self, inst: &mut Instance, o: usize) {
        let prefab = inst.prefab.clone();
        let subtree: Vec<usize> = std::iter::once(o).chain(inst.descendants(o)).collect();
        // The clone's Transforms are the pose only until the bodies first move.
        // A PhysicalBehaviour's Transform is kept live (write-back, refresh_nested), so it gives the world pose then.
        let mut live = None;
        if let Some(&b) = inst.bodies.get(&o) {
            let (pos, xf) = (inst.position(o), self.world.body(b).xf.p);
            if pos[0] != xf.x || pos[1] != xf.y {
                let p = self.physicals.iter().position(|p| inst.objects[o].components.contains(&p.id)).filter(|&p| self.physicals[p].transform.local.is_some());
                // A Rigidbody2D without a PhysicalBehaviour: its Transform node is kept live.
                let k = self.node_rigidbodies.iter().position(|n| self.transform_nodes[n.node].id == inst.objects[o].components[0]);
                match (p, k) {
                    (Some(p), _) => {
                        let t = &self.physicals[p].transform;
                        live = Some((t.position, t.rotation, self.lossy_scale(p)));
                    }
                    (None, Some(k)) => {
                        let (position, rotation) = self.node_world(self.node_rigidbodies[k].node);
                        live = Some((position, rotation, self.owner_lossy_scale(BodyOwner::Node(k))));
                    }
                    (None, None) => unimplemented!("mod-only: {}: Transform.SetParent after its body moved (the clone's Transforms are stale)", prefab.objects[o].name),
                }
            }
        }
        let bodies: Vec<_> = subtree.iter().filter_map(|x| inst.bodies.get(x).copied()).collect();
        for (&(x, _), fixtures) in &inst.fixtures {
            if subtree.contains(&x) && fixtures.first().is_some_and(|&f| !bodies.contains(&self.world.fixture(f).body)) {
                unimplemented!("mod-only: {}: SetParent(null) moves a Collider2D off its Rigidbody2D (Collider2D::ParentChanged)", prefab.objects[x].name);
            }
        }
        // A bare Transform below a PhysicalBehaviour (Adhesive Coupler's Killzone): its world pose through the live
        // physical's chain, as attached_clone_set_parent_null reads it (the clone's own Transforms are stale).
        let own_physical = self.physicals.iter().any(|p| inst.objects[o].components.contains(&p.id));
        let chained = live.is_none() && !own_physical && inst.objects[o].parent.is_some_and(|q| {
            let mut at = Some(q);
            while let Some(x) = at {
                if self.physicals.iter().any(|p| !p.destroyed && inst.objects[x].components.contains(&p.id)) {
                    return true;
                }
                at = inst.objects[x].parent;
            }
            false
        });
        if chained {
            use crate::unity::transform::{global_position, global_rotation, lossy_scale};
            let chain = self.transform_chain(inst, o);
            let (mut v, mut r) = (chain[0].t, chain[0].q);
            for x in &chain[1..] {
                (v, r) = (global_position(x, v), global_rotation(x, r));
            }
            let s = lossy_scale(&chain, r);
            live = Some(([v[0], v[1], v[2]], r, s));
        }
        match live {
            Some(world) => inst.set_parent_null_at(o, world),
            None => inst.set_parent_null(o),
        }
        // OnTransformParentChanged (its children get it too, but their parent and so their parentRenderer stay the same).
        self.sorting_layer_child_parent_changed(inst, o);
        let transform = inst.objects[o].components[0];
        let physical = self.physicals.iter().position(|p| inst.objects[o].components.contains(&p.id));
        let node = self.transform_nodes.iter().position(|n| n.id == transform);
        if let Some(p) = physical {
            self.physicals[p].parent = None;
            self.physicals[p].transform = pose(inst, o);
        }
        if let Some(n) = node {
            self.transform_nodes[n].parent = None;
            self.transform_nodes[n].trs = inst.objects[o].trs;
        }
        // (Through the live chain the detached pose is right, and a bare Transform stays where it is left: its scripted
        // sprites read a correct pose.)
        if physical.is_none() && node.is_none() && subtree.iter().any(|&x| prefab.objects[x].components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_)) || (!chained && matches!(c, Component::SpriteRenderer(_)) && prefab.objects[x].components.iter().any(|(_, k)| matches!(k, Component::Script { class, .. } if class != "SerialisableIdentity"))))) {
            // A sprite without a script on its object (SerialisableIdentity only names it for saves) is presentation:
            // nothing reads its pose.
            unimplemented!("mod-only: {}: SetParent(null) of a bare Transform above colliders or scripted sprites", prefab.objects[o].name);
        }
        self.reparented_rigidbodies(inst, o);
    }

    /// The rigidbody systems' reaction to Transform.SetParent(null) of clone object `o`: for each Rigidbody2D below it,
    /// CheckForDrivenByParentRigidbody(null) and no interpolation.
    pub(super) fn reparented_rigidbodies(&mut self, inst: &Instance, o: usize) {
        let prefab = inst.prefab.clone();
        for x in std::iter::once(o).chain(inst.descendants(o)) {
            let Some(k) = prefab.objects[x].components.iter().position(|(_, c)| matches!(c, Component::Rigidbody2D(_))) else { continue };
            if !inst.bodies.contains_key(&x) {
                continue;
            }
            let id = inst.objects[x].components[k];
            let owner = self.rigidbody_owner(id).expect("a Rigidbody2D outside the arenas");
            // CheckForDrivenByParentRigidbody(null): the nearest ancestor Rigidbody2D drives a non-static one.
            let driven_by = self.rigidbody2d_driven_by(inst, &prefab, x, self.owner_rigidbody(owner).bodyType);
            self.set_driven_by(owner, driven_by);
            self.owner_rigidbody_mut(owner).movement.interpolating = false;
            self.rigidbody_hierarchy_changed = true;
        }
    }

    pub(super) fn refresh_transforms(&mut self, inst: &Instance, linked: &Linked) {
        for (&o, &n) in &linked.nodes {
            self.transform_nodes[n].trs = inst.objects[o].trs;
        }
        for (&o, &p) in &linked.physicals {
            self.physicals[p].transform = pose(inst, o);
        }
        if let Some(person) = linked.person {
            let s = inst.objects[0].trs.s;
            self.people[person].localScale = [s[0], s[1], s[2]];
        }
    }

    /// CatalogBehaviour.Update (no pause menu, dialog or UI block): releasing "spawnRight" spawns the selected contraption (its ObjectStates pasted
    /// at Global.MousePosition, Spawn(Contraption)) or SelectedItem facing right, else releasing "spawnLeft" spawns it flipped.
    pub fn catalog_update(&mut self) {
        if self.catalog_selected.is_none() && self.catalog_contraption.is_none() {
            return;
        }
        let up = |k: &str| self.input.controls.get(k).is_some_and(|c| c.up);
        let flipped = match (up("spawnRight"), up("spawnLeft")) {
            (true, _) => false,
            (false, true) => true,
            _ => return,
        };
        if let Some((states, name)) = self.catalog_contraption.clone() {
            let at = self.MousePosition;
            self.paste_states(&states, at, flipped, &name);
            // Spawn(Contraption): SelectedContraptionMetaData.DisplayName, the contraption's ObjectStates, flipped.
            crate::ui::uifn("CatalogBehaviour.Spawn", || format!("{name} {} {}", states.len(), if flipped { "True" } else { "False" }));
            return;
        }
        let item = self.catalog_selected.clone().expect("a selected item");
        let mut db = self.assets.take().expect("CatalogBehaviour.Update without the game's data");
        let spawned = self.catalog_spawn(&mut db, &item, self.MousePosition, flipped);
        self.assets = Some(db);
        match spawned {
            Ok((inst, _)) => {
                // Spawn(SpawnableAsset): the asset and the spawned root's localScale.x.
                let x = inst.objects[0].trs.s[0];
                crate::ui::uifn("CatalogBehaviour.Spawn", || format!("{item} {}", super::menu::cs_float(x)));
                self.unsynced.push(inst)
            }
            Err(e) => panic!("CatalogBehaviour.Spawn({item}): {e}"),
        }
    }

    /// A FirearmBehaviour's serialized fields and what its Awake reads from assets: the Cartridge, the sound and
    /// inaccuracy lists, the layers, the MuzzleFlashEffectOverride and the ids the BulletCasing and MuzzleSmoke
    /// Instantiates take. The links to the physical object, the component id and enabled are the caller's.
    /// The ActOnCollide of a recorded item (an object the recording already holds): the fields, and Actions of
    /// presentation calls only (audio and particles run nothing here). `index` is the component's index in the prefab
    /// object, `physical` the item's PhysicalBehaviour.
    pub(super) fn act_on_collide_record(&mut self, prefab: &Prefab, o: usize, physical: usize, firearm: usize) -> Result<(), String> {
        let Some((c, enabled, v)) = script(prefab, o, "ActOnCollide") else { return Ok(()) };
        let file = prefab.objects[o].game_object.file;
        let mut x = super::acts::ActOnCollide::default();
        load_values(x.fields(), v)?;
        if field(v, "Collider")?.pptr().is_some_and(|p| !p.is_null()) {
            unimplemented!("mod-only: ActOnCollide.Collider on a recorded item");
        }
        for call in invoked_calls(field(v, "Actions")?) {
            let method = string(call, "m_MethodName")?;
            let class_of = |to: usize, tc: usize| match &prefab.objects[to].components[tc].1 {
                Component::Script { class, .. } => class.as_str(),
                _ => "",
            };
            match call_target(prefab, file, call) {
                // FirearmBehaviour.Shoot of the item's own FirearmBehaviour.
                Some((to, tc)) if to == o && class_of(to, tc) == "FirearmBehaviour" && method == "Shoot" => x.targets.push(super::acts::ActTarget::Shoot(firearm)),
                _ if presentation_call(prefab, file, call) => {}
                _ => unimplemented!("mod-only: ActOnCollide.Actions on a recorded item: {method}"),
            }
        }
        (x.enabled, x.physical, x.index) = (enabled, Some(physical), c);
        x.after_physical = script(prefab, o, "PhysicalBehaviour").is_some_and(|(pc, _, _)| pc < c);
        // Component ids count down by 2 in prefab component order from the PhysicalBehaviour's.
        let pc = script(prefab, o, "PhysicalBehaviour").ok_or("a recorded item without a PhysicalBehaviour")?.0;
        x.id = self.physicals[physical].id - 2 * (c as i32 - pc as i32);
        self.act_on_collides.push(x);
        Ok(())
    }

    /// A script's `ParticleSystem` field: the system's GameObject and where its Transform hangs (a physical, a node or the root).
    fn add_embedded_ps(&mut self, l: &Linker, prefab: &Prefab, inst: &Instance, reference: &Value, owner: i32, drive: super::EmbeddedDrive) -> Result<(), String> {
        let r = reference.pptr().ok_or("ParticleSystem")?;
        // A null ParticleSystem draws nothing (the script's own NullReferenceException is its row's).
        let Some(ps) = l.object_of(r)? else { return Ok(()) };
        self.add_embedded_ps_at(l, prefab, inst, ps, owner, drive);
        Ok(())
    }

    /// The ParticleSystem on object `ps`.
    fn add_embedded_ps_at(&mut self, l: &Linker, prefab: &Prefab, inst: &Instance, ps: usize, owner: i32, drive: super::EmbeddedDrive) {
        let (mut chain, mut up) = (Vec::new(), Some(ps));
        let anchor = loop {
            let Some(t) = up else { break BareAnchor::Root };
            if let Some(&p) = l.arenas.physical.get(&t) {
                break BareAnchor::Physical(p);
            }
            if let Some(&i) = l.arenas.node.get(&t) {
                break BareAnchor::Node(i);
            }
            chain.push(inst.objects[t].trs);
            up = inst.objects[t].parent;
        };
        self.embedded_ps.push(super::EmbeddedPs { go: prefab.objects[ps].game_object, anchor, chain, owner, drive, shape_texture: None, awake: true, emission: None });
    }

    pub(super) fn firearm_from_fields(db: &mut AssetDb, file: usize, v: &Value) -> Result<super::firearm::Firearm, String> {
        let mut fa = super::firearm::Firearm::default();
        load_values(fa.fields(), v)?;
        let cartridge = field(v, "Cartridge")?.pptr().ok_or("Cartridge")?;
        let Some(cartridge) = db.resolve(file, cartridge) else { unimplemented!("mod-only: FirearmBehaviour.Awake with a null Cartridge (NullReferenceException)") };
        let (_, _, cv) = db.mono(cartridge)?.ok_or("Cartridge is not a MonoBehaviour")?;
        fa.Cartridge = super::ballistics::Cartridge::read(&cv)?;
        fa.ShotSounds = array(v, "ShotSounds")?.len();
        fa.InaccuracyMultipliers = array(v, "InaccuracyMultipliers")?.iter().map(|x| x.f32().ok_or("InaccuracyMultipliers")).collect::<Result<_, _>>()?;
        fa.LayersToHit = field(field(v, "LayersToHit")?, "m_Bits")?.i64().ok_or("LayersToHit.m_Bits")? as u32;
        let effect = field(v, "MuzzleFlashEffectOverride")?.pptr().ok_or("MuzzleFlashEffectOverride")?;
        if !effect.is_null() {
            let r = db.resolve(file, effect).ok_or("MuzzleFlashEffectOverride outside the data")?;
            let physics = db.prefab(r)?.objects.iter().any(|x| x.components.iter().any(|(_, c)| matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_))));
            if physics {
                unimplemented!("mod-only: FirearmBehaviour: a MuzzleFlashEffectOverride with physics (its pose)");
            }
            fa.MuzzleFlashEffectOverride = Some(r);
        }
        fa.casing_prefab = db.resource("Prefabs/BulletCasing", 1)?;
        fa.casing_clone = fa.casing_prefab.map(|r| db.prefab(r)).transpose()?.map(std::sync::Arc::new);
        // The ids an Instantiate of a Resources prefab takes: each GameObject and its components.
        let mut clone_ids = |path: &str| -> Result<usize, String> {
            let r = db.resource(path, 1)?.ok_or_else(|| format!("Resources.Load({path}) found no GameObject"))?;
            Ok(db.prefab(r)?.objects.iter().map(|x| 1 + x.components.len()).sum())
        };
        fa.casing_ids = clone_ids("Prefabs/BulletCasing")?;
        fa.smoke_ids = if fa.MuzzleFlashEffectOverride.is_some() { 0 } else { clone_ids("Prefabs/MuzzleSmoke")? };
        Ok(fa)
    }

    /// CatalogBehaviour.Spawn(SelectedItem, flipped) at Global.MousePosition: PerformInstantiation (no
    /// InstantiateOverride without mods), then AddComponent DeregisterBehaviour, TexturePackApplier,
    /// AudioSourceTimeScaleBehaviour and SerialiseInstructions, and the asset's name. SpawnedGameObjects, the undo
    /// action and the stats are not simulation state; DeregisterBehaviour and TexturePackApplier have no messages
    /// and AudioSourceTimeScaleBehaviour.Start only registers audio.
    pub fn catalog_spawn(&mut self, db: &mut AssetDb, asset: &str, position: [f32; 3], flipped: bool) -> Result<(Instance, Linked), String> {
        let t0 = std::time::Instant::now();
        let prefab = Self::catalog_prefab(db, asset)?;
        if std::env::var_os("PP_SPAWN_BENCH").is_some() { eprintln!("BENCH   get_spawnable {:.2} ms", t0.elapsed().as_secs_f64() * 1000.0); }
        // CatalogBehaviour.PerformInstantiation: e.InstantiateOverride is null without mods, so Object.Instantiate(SelectedItem.Prefab, position,
        // Quaternion.Euler(0, 0, 0)) (SelectedItem is `asset`).
        let (mut inst, linked) = self.spawn(db, &prefab, position, [0.0, 0.0, 0.0, 1.0], None)?;
        if flipped {
            let s = inst.objects[0].trs.s;
            inst.set_local_scale(0, [s[0] * -1.0, s[1], s[2]]);
            self.refresh_transforms(&inst, &linked);
        }
        self.spawnable_asset_components(&prefab, &linked, asset, 4);
        // SpawnedGameObjects.Add (not simulation state); UndoControllerBehaviour.RegisterAction(new ObjectCreationAction(gameObject)),
        // whose DeregisterBehaviour (added above) drops it again when the object is destroyed.
        let root = inst.objects[0].components[0];
        self.deregister_behaviours.insert(root);
        self.undo_register_action(super::undo::ObjectCreationAction { displayName: asset.to_string(), RelevantObjects: vec![root] });
        super::stats::increment_integer(super::stats::TOTAL_SPAWNED_ITEMS, 1);
        super::stats::with_stats(|s| s.increment("TOTAL_SPAWNED_ITEMS", 1.0));
        // ModdingApi.Events.OnItemSpawned.Invoke(new UserSpawnEventArgs(gameObject, e)).
        let args = super::mods::UserSpawnEventArgs::new(root, Some(asset.to_string()));
        super::mods::mod_event_invoke(&super::mods::mod_api::OnItemSpawned, args.Instance, &args);
        Ok((inst, linked))
    }

    /// PP_SPAWN_BENCH=list: every catalog item name.
    pub fn catalog_names(db: &mut AssetDb) -> Result<Vec<String>, String> {
        let mut names = Vec::new();
        for r in db.resources_in("SpawnableAssets", 114)? {
            names.push(db.mono(r)?.ok_or("SpawnableAsset")?.1.name.clone());
        }
        Ok(names)
    }

    /// The fn probe's spawn of a class no catalog item carries (Recorder.cs GenRun, ClassPrefab): the root of a prefab
    /// asset (no level file's object) holding a `class` script, with at most 64 objects, named `name`. Same-named ones (two "Pool"s hold a WaterBehaviour) are told apart by the recorded clone: its
    /// objects' names (`recorded`, depth first) hold the prefab's in order. More than one left is an error.
    pub fn class_prefab(db: &mut AssetDb, class: &str, name: &str, recorded: &[String]) -> Result<Prefab, String> {
        use crate::assets::classes::{MonoBehaviour, Transform};
        let mut found = std::collections::BTreeSet::new();
        let mut roots = Vec::new();
        for r in db.find_scripts(class)? {
            if db.files[r.file].path.file_name().and_then(|f| f.to_str()).is_some_and(|f| f.starts_with("level")) {
                continue;
            }
            let Some(own) = db.resolve(r.file, MonoBehaviour::read(db.data(r))?.game_object) else { continue };
            let mut root = own;
            loop {
                let go = db.game_object(root)?;
                let t = go.components.iter().filter_map(|c| db.resolve(root.file, *c)).find(|&c| db.class(c) == 4).ok_or("GameObject without a Transform")?;
                let father = Transform::read(db.data(t))?.father;
                match db.resolve(t.file, father) {
                    Some(f) => root = db.resolve(f.file, Transform::read(db.data(f))?.game_object).ok_or("Transform without a GameObject")?,
                    None => break,
                }
            }
            let pick = root;
            roots.push(format!("{:?} ({} objects, {})", db.game_object(pick)?.name, db.prefab_cached(root)?.objects.len(), db.files[root.file].path.file_name().and_then(|f| f.to_str()).unwrap_or("?")));
            if db.prefab_cached(root)?.objects.len() > 64 {
                continue;
            }
            // (An Instantiate'd one keeps its "(Clone)" name: PyroPowerTool.Awake's entity.)
            let n = db.game_object(pick)?.name.clone();
            if n == name || n == name.replace('_', " ") || name.strip_suffix("(Clone)") == Some(n.as_str()) {
                found.insert(pick);
            }
        }
        if found.len() > 1 {
            let mut fits = std::collections::BTreeSet::new();
            for &f in &found {
                let p = db.prefab_cached(f)?;
                let mut rec = recorded.iter().skip(1);
                if p.objects.iter().skip(1).all(|o| rec.any(|r| *r == o.name.replace(' ', "_"))) {
                    fits.insert(f);
                }
            }
            found = fits;
        }
        match found.len() {
            0 => Err(format!("no prefab {name:?} holding a {class} (roots holding one: {})", roots.join(", "))),
            1 => Ok((*db.prefab_cached(*found.first().unwrap())?).clone()),
            n => Err(format!("{n} prefabs {name:?} hold a {class}")),
        }
    }

    /// The prefab of the catalog item named `asset` (its SpawnableAsset's Prefab), found as CatalogBehaviour.GetSpawnable does.
    /// Older recorders kept a clone's own name ("Person(Clone)", the Human's prefab root): the one catalog item whose
    /// prefab root has that name.
    pub fn catalog_prefab(db: &mut AssetDb, asset: &str) -> Result<Prefab, String> {
        let err = match Self::get_spawnable(db, asset) {
            Ok(p) => return Ok(p),
            Err(e) => e,
        };
        let Some(root_name) = asset.strip_suffix("(Clone)") else { return Err(err) };
        let mut roots: Vec<ObjRef> = db.catalog_lookup.as_ref().map(|(n, _)| n.values().copied().collect()).unwrap_or_default();
        roots.sort_by_key(|r| (r.file, r.path_id));
        roots.dedup();
        let mut found = Vec::new();
        for r in roots {
            let p = db.prefab_cached(r)?;
            if p.objects.first().is_some_and(|o| o.name == root_name) {
                found.push(p);
            }
        }
        match found.len() {
            1 => Ok((*found[0]).clone()),
            n => Err(format!("{err} ({n} catalog prefabs named {root_name:?})")),
        }
    }

    /// CatalogBehaviour.GetSpawnable: the Prefab of the SpawnableAsset in spawnableByName (CatalogBehaviour.Populate: Catalog.Items =
    /// Resources.LoadAll<SpawnableAsset>("SpawnableAssets"), each by its name; the serialized CatalogData.Items lacks the Railgun and others), else in spawnableByPriorName (the assets with HasPriorName, by PriorName), else none.
    /// Populate throws when an asset with HasPriorName has a blank PriorName. No mod adds a spawnable (CatalogBehaviour.ApplyModification
    /// and GetModOfSpawnable are the mod loader's: spawnableByMod stays empty).
    pub fn get_spawnable(db: &mut AssetDb, asset: &str) -> Result<Prefab, String> {
        if db.catalog_lookup.is_none() {
            let (mut by_name, mut by_prior) = (std::collections::HashMap::new(), std::collections::HashMap::new());
            for r in db.resources_in("SpawnableAssets", 114)? {
                let (_, mb, v) = db.mono(r)?.ok_or("SpawnableAsset")?;
                let prefab = |db: &mut AssetDb| -> Result<_, String> { db.resolve(r.file, v.get("Prefab").and_then(Value::pptr).ok_or("SpawnableAsset.Prefab")?).ok_or_else(|| "SpawnableAsset.Prefab".to_string()) };
                if v.get("HasPriorName").and_then(Value::bool).unwrap_or(false) {
                    let prior = match v.get("PriorName") {
                        Some(Value::Str(s)) => s.clone(),
                        _ => String::new(),
                    };
                    if prior.trim().is_empty() {
                        crate::game::menu::throw(&format!("Exception: {} has no prior name, but it indicates that it should have one", mb.name));
                    }
                    if !by_prior.contains_key(&prior) {
                        by_prior.insert(prior, prefab(db)?);
                    }
                }
                if !by_name.contains_key(&mb.name) {
                    by_name.insert(mb.name.clone(), prefab(db)?);
                }
            }
            Self::build_spawnable_migrations(db)?;
            // extras: new items that are not in the real game join the lookup (src/extras_hook.rs).
            crate::extras_hook::register(db, &mut by_name)?;
            db.catalog_lookup = Some((by_name, by_prior));
        }
        let (by_name, by_prior) = db.catalog_lookup.as_ref().unwrap();
        // Recordings name an item's GameObject with its spaces as underscores ("Empty_Syringe").
        let spaced = asset.replace('_', " ");
        let root = by_name.get(asset).or_else(|| by_prior.get(asset)).or_else(|| by_name.get(&spaced)).or_else(|| by_prior.get(&spaced)).copied().ok_or_else(|| format!("no catalog item {asset:?}"))?;
        Ok((*db.prefab_cached(root)?).clone())
    }

    /// The name of the SpawnableAsset GetSpawnable finds for `asset`: a prior name gives the asset that now has the prefab
    /// (ObjectState.CreateNew names the object spawnableAsset.name: a "Handcannon" save makes a "Desert Eagle").
    pub fn spawnable_name(db: &mut AssetDb, asset: &str) -> String {
        let _ = Self::get_spawnable(db, "");
        let Some((by_name, by_prior)) = db.catalog_lookup.as_ref() else { return asset.into() };
        let spaced = asset.replace('_', " ");
        if by_name.contains_key(asset) || by_name.contains_key(&spaced) {
            return asset.into();
        }
        let Some(r) = by_prior.get(asset).or_else(|| by_prior.get(&spaced)) else { return asset.into() };
        by_name.iter().filter(|(_, v)| *v == r).map(|(k, _)| k.clone()).min().unwrap_or_else(|| asset.into())
    }

    /// `ModdingApi.FindSpawnable(name)?.MigrationEvents` for every name GetSpawnable knows, each event with its ToSpawnInstead's name (an event
    /// without one never applies), into `contraption::MIGRATIONS`. FindSpawnable first swaps the found asset for the target of its first event
    /// applicable to "1.28".
    pub(crate) fn build_spawnable_migrations(db: &mut AssetDb) -> Result<(), String> {
        type Events = Vec<(super::contraption::MigrationEvent, String)>;
        let mut assets: Vec<(String, Option<String>, Events)> = Vec::new();
        for r in db.resources_in("SpawnableAssets", 114)? {
            let (_, mb, v) = db.mono(r)?.ok_or("SpawnableAsset")?;
            let prior = (v.get("HasPriorName").and_then(Value::bool) == Some(true)).then(|| match v.get("PriorName") {
                Some(Value::Str(s)) => s.clone(),
                _ => String::new(),
            });
            let mut events = Events::new();
            for e in v.get("MigrationEvents").and_then(Value::array).unwrap_or(&[]) {
                let Some(target) = e.get("ToSpawnInstead").and_then(Value::pptr).and_then(|p| db.resolve(r.file, p)) else { continue };
                let (_, to, _) = db.mono(target)?.ok_or("MigrationEvent.ToSpawnInstead")?;
                let Some(Value::Str(version)) = e.get("Version") else { return Err("MigrationEvent.Version".into()) };
                use super::contraption::MigrationCondition::*;
                let condition = [OlderThan, OlderThanOrEqualTo, EqualTo, NewerThan, NewerThanOrEqualTo][e.get("VersionCondition").and_then(Value::i64).unwrap_or(0) as usize];
                events.push((super::contraption::MigrationEvent { Version: version.clone(), VersionCondition: condition }, to.name));
            }
            assets.push((mb.name, prior, events));
        }
        let find = |name: &str| assets.iter().find(|a| a.0 == name).or_else(|| assets.iter().find(|a| a.1.as_deref() == Some(name)));
        let mut table = std::collections::HashMap::new();
        for name in assets.iter().flat_map(|a| std::iter::once(a.0.clone()).chain(a.1.clone())) {
            let Some(mut asset) = find(&name) else { continue };
            if let Some((_, to)) = asset.2.iter().find(|(e, _)| e.IsApplicable(super::contraption::current_version(), true)) {
                asset = assets.iter().find(|a| a.0 == *to).expect("a migration target is a SpawnableAsset");
            }
            table.entry(name).or_insert_with(|| asset.2.clone());
        }
        let _ = super::contraption::MIGRATIONS.set(table);
        Ok(())
    }

    /// Load-time work Unity has done before play: the catalog's lookups and prefabs, and its sprites' textures decoded.
    pub fn preload_catalog(db: &mut AssetDb) {
        let _ = Self::get_spawnable(db, "");
        let Some((by_name, by_prior)) = db.catalog_lookup.clone() else { return };
        let mut roots: Vec<_> = by_name.into_values().chain(by_prior.into_values()).collect();
        roots.sort_by_key(|r| (r.file, r.path_id));
        roots.dedup();
        let mut textures = std::collections::BTreeSet::new();
        for root in roots {
            let Ok(prefab) = db.prefab_cached(root) else { continue };
            for o in &prefab.objects {
                for (_, c) in &o.components {
                    let Component::SpriteRenderer(sr) = c else { continue };
                    let Some(r) = db.resolve(o.game_object.file, sr.sprite) else { continue };
                    let Some(t) = db.sprite(r).ok().and_then(|s| db.resolve(r.file, s.texture)) else { continue };
                    textures.insert((t.file, t.path_id));
                }
            }
        }
        // The catalog's sprite textures first, then every other Texture2D in the data (particle sheets, materials', UI): any may be
        // a first sight mid-game, so all are decoded here and none stalls a frame (a cache: what is drawn does not change).
        let mut list: Vec<_> = textures.into_iter().map(|(file, path_id)| ObjRef { file, path_id }).collect();
        let seen: std::collections::HashSet<_> = list.iter().map(|r| (r.file, r.path_id)).collect();
        for (file, f) in db.files.iter().enumerate() {
            list.extend(f.objects.iter().filter(|o| o.class_id == 28 && !seen.contains(&(file, o.path_id))).map(|o| ObjRef { file, path_id: o.path_id }));
        }
        let t = std::time::Instant::now();
        db.preload_textures(&list);
        if std::env::var_os("PP_SPAWN_BENCH").is_some() { eprintln!("BENCH preload {} textures {:.0} ms", list.len(), t.elapsed().as_secs_f64() * 1000.0); }
    }

    /// The `added` components a SpawnableAsset clone gets (AudioSourceTimeScaleBehaviour, TexturePackApplier and
    /// SerialiseInstructions among them) and its gameObject.name.
    pub(super) fn spawnable_asset_components(&mut self, prefab: &Prefab, linked: &Linked, asset: &str, added: usize) {
        for _ in 0..added {
            self.allocate_instance_id();
        }
        // The root's AudioSourceTimeScaleBehaviour, which PhysicalBehaviour.Start's GetComponentInParent finds.
        match (linked.physicals.get(&0), linked.nodes.get(&0)) {
            (Some(&p), _) => self.physicals[p].audio_time_scale = true,
            (None, Some(&n)) => self.audio_time_scale_roots.push(n),
            (None, None) => unimplemented!("mod-only: a catalog item whose root is neither physical nor a parent of physicals"),
        }
        // gameObject.name = e.name
        let root = match (linked.physicals.get(&0), linked.nodes.get(&0)) {
            (Some(&p), _) => self.physicals[p].transform_id,
            (_, Some(&n)) => self.transform_nodes[n].id,
            _ => unreachable!(),
        };
        // Its Start's Global.AddAudioSource of every AudioSource below (IsAmbience false): their pitch follows
        // Time.timeScale.
        self.pending_starts.push(super::misc::PendingStart::AudioTimeScale(root, false));
        self.object_names.insert(root, asset.to_string());
        if let Some(&p) = linked.physicals.get(&0) {
            self.physicals[p].name = asset.to_string();
        }
        if let Some(p) = linked.person {
            self.people[p].name = asset.to_string();
        }
        // SerialisableIdentity.Regenerate: UniqueIdentity = Guid.NewGuid() and its inspector string, not read by the simulation (saves only)
        // SerialiseInstructions.Start: a SerialisableIdentity on the root and on every Transform below without an
        // Optout, unless one is there (what Starts add first, the outlines and particles, all carry Optout).
        let has = |o: &PrefabObject, class: &str| o.components.iter().any(|(_, c)| matches!(c, Component::Script { class: k, .. } if k == class));
        let identities = prefab.objects.iter().enumerate().filter(|&(o, ob)| !has(ob, "SerialisableIdentity") && (o == 0 || !has(ob, "Optout"))).count();
        self.pending_starts.push(super::misc::PendingStart::SerialiseInstructions(identities, root));
    }

    /// GameObject.SetActive(false) on object `o` while its clone is being awoken: the OnDisable of the scripts below it
    /// whose OnEnable already ran.
    fn awake_set_inactive(&mut self, prefab: &Prefab, inst: &mut Instance, linked: &Linked, enabled_behaviours: &mut std::collections::HashSet<(usize, usize)>, o: usize) {
        for (d, k) in self.set_active(inst, o, false) {
            if !enabled_behaviours.remove(&(d, k)) {
                continue;
            }
            let Component::Script { class, .. } = &prefab.objects[d].components[k].1 else { unreachable!() };
            match class.as_str() {
                "PhysicalBehaviour" => self.physical_on_disable(linked.physicals[&d]),
                "LEDBulbBehaviour" => self.led_on_disable(linked.leds[&d]),
                "DetectorBehaviour" => self.detector_on_disable(linked.detectors[&d]),
                "GeneratorBehaviour" => self.generator_on_disable(linked.generators[&d]),
                "SingleFloodlightBehaviour" => self.floodlight_on_disable(linked.floodlights[&d], inst),
                "SpikeGrenadeBehaviour" => self.spike_grenade_on_disable(linked.spike_grenades[&d]),
                "TemperatureRayGunBehaviour" => self.ray_gun_on_disable(linked.ray_guns[&d]),
                "ThrusterbedBehaviour" => self.thrusterbed_on_disable(linked.thrusterbeds[&d]),
                "MetronomeBehaviour" => self.metronome_on_disable(linked.metronomes[&d]),
                "PhysicsGunBehaviour" => self.physics_gun_on_disable(linked.physics_guns[&d], inst),
                "TimedGateBehaviour" => self.timed_gate_on_disable(linked.timed_gates[&d]),
                "LagboxBehaviour" => self.lagbox_on_disable(linked.lagboxes[&d]),
                "GateBehaviour" => self.gate_on_disable(linked.gates[&d]),
                "HoverThrusterBehaviour" => self.hover_thruster_on_disable(linked.hover_thrusters[&d]),
                "WinchBehaviour" => self.winch_on_disable(linked.winches[&d]),
                "ConveyorBeltBehaviour" => self.conveyor_on_disable(linked.conveyors[&d]),
                "ConveyorBeltEffector" => {}
                "JukeboxBehaviour" => self.jukebox_on_disable(linked.jukeboxes[&d]),
                "TransmitterBehaviour" => self.transmitter_on_disable(linked.transmitters[&d]),
                "RayCannonBehaviour" => self.ray_cannon_on_disable(linked.ray_cannons[&d]),
                "RailgunBehaviour" => self.railgun_on_disable(linked.railguns[&d]),
                "DefibrillatorBehaviour" => self.defibrillator_on_disable(linked.defibrillators[&d]),
                "CardiopulmonaryBypassMachineBehaviour" => self.cardiopulmonary_bypass_on_disable(linked.cardiopulmonary_bypasses[&d]),
                "HeartMonitorBehaviour" => self.heart_monitor_on_disable(linked.heart_monitors[&d]),
                "ImmobilityFieldBehaviour" => self.immobility_field_on_disable(linked.immobility_fields[&d], inst),
                "LightningGunBehaviour" => self.lightning_gun_on_disable(linked.lightning_guns[&d]),
                "CeilingTurretBehaviour" => self.ceiling_turret_on_disable(linked.ceiling_turrets[&d]),
                "PointerBehaviour" => self.pointer_on_disable(linked.pointers[&d]),
                "TeslaCoilBehaviour" => self.unity_callback(|g| g.tesla_coil_on_disable(linked.tesla_coils[&d])),
                // OnDisable -> OnDestroy: clears softConnections, empty before the first FixedUpdate.
                "SharpOnAllSidesBehaviour" => {}
                "InfraredThermometer" => self.infrared_on_disable(linked.infrared_thermometers[&d]),
                "ActivationToggleBehaviour" => self.activation_toggle_on_disable(linked.activation_toggles[&d]),
                "DisplayBehaviour" => self.display_on_disable(linked.displays[&d], inst),
                "ButtonBehaviour" => self.button_on_disable(linked.buttons[&d]),
                "LaserReceiverBehaviour" => self.laser_receiver_on_disable(linked.laser_receivers[&d]),
                "LaserBehaviour" => self.laser_on_disable(linked.lasers[&d]),
                "JavelinBehaviour" => self.javelin_on_disable(linked.javelins[&d]),
                "BeamformerBehaviour" => self.beamformer_on_disable(linked.beamformers[&d]),
                "ThermometerBehaviour" => self.thermometer_on_disable(linked.thermometers[&d]),
                "KeyTriggerBehaviour" => self.key_trigger_on_disable(linked.key_triggers[&d]),
                // No OnDisable; TextMeshPro's only touches its mesh and the font atlas.
                "ActivateIfTutorial" | "ActivateIfFancyEffects" | "AudioSourceTimeScaleBehaviour" | "DetachFromParentOnStart" | "DebrisComponent" | "Optout" | "MaintainScaleSignBehaviour" | "TextMeshProControlReplacer" | "TextMeshPro" => {}
                name if super::translated::CLASSES.contains(&name) => self.tr_on_disable(linked.translated[&(d, k)]),
                "AntennaReceiverBehaviour" => self.antenna_receiver_on_disable(linked.antenna_receivers[&d]),
                c if super::lifecycle::LIFECYCLE.iter().any(|(k, m)| *k == c && !m.contains(&"OnDisable")) => {}
                _ => unimplemented!("mod-only: {class}.OnDisable"),
            }
        }
        for (&d, &p) in &linked.physicals {
            self.physicals[p].active = inst.objects[d].active;
        }
        for (&d, &w) in &linked.waters {
            self.waters[w].active = inst.objects[d].active;
        }
    }

    /// Awake of each queue 0x14 behaviour, in `instantiate_native`'s order, skipping objects inactive by then.
    /// The classes left out have no Awake that touches simulation state (ConnectedNodeBehaviour, GripBehaviour
    /// and GoreStringBehaviour only read components; MapConfig, MapLightBehaviour and WindControllerBehaviour
    /// only fetch components), and none of these scripts has OnEnable.
    /// `db`: the game's data when the caller has taken it out of `assets` (a spawn); None when it is in place.
    pub fn awake_behaviours(&mut self, mut db: Option<&mut AssetDb>, prefab: &Prefab, inst: &mut Instance, linked: &Linked, order: &[(usize, usize)]) {
        // The behaviours whose OnEnable has run (awake and enabled), for OnDisable.
        let mut enabled_behaviours = std::collections::HashSet::new();
        // The objects whose scripts have run Awake: one deactivated (by an earlier Awake) before any of its scripts
        // awoke has its Awake and OnEnable at its first SetActive(true) instead.
        let mut awoken = std::collections::HashSet::new();
        let mut ctx = None;
        for &(o, c) in order {
            if !inst.objects[o].active {
                if !awoken.contains(&o) {
                    inst.objects[o].activated = false;
                }
                continue;
            }
            awoken.insert(o);
            let Component::Script { class, enabled, execution_order, fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
            check_rewritten_at(prefab, o, class);
            if *enabled {
                enabled_behaviours.insert((o, c));
            }
            if matches!(class.as_str(), "IndustrialLiftBehaviour" | "ReactorConduitBehaviour" | "TeslaCoilBehaviour") {
                let x = ctx.get_or_insert_with(|| std::sync::Arc::new((prefab.clone(), inst.clone(), linked.clone()))).clone();
                self.awake_ctx.insert(inst.objects[o].components[c], (x, o, c));
            }
            let limb = || linked.limbs[&o];
            // Unity runs Awake and OnEnable per component: a managed exception ends only the message it was thrown in.
            let caught = super::menu::catch_managed(|| match class.as_str() {
                // TextMeshPro.Awake at a first activation takes its instance ids (MeshFilter, mesh) then (one active at
                // Instantiate took them in add_world_text).
                "TextMeshPro" => {
                    let id = inst.objects[o].components[c];
                    if let Some(t) = self.world_texts.iter().position(|t| t.id == id && !t.awoken) {
                        self.world_texts[t].awoken = true;
                        self.allocate_instance_ids(self.world_texts[t].awake_ids);
                    }
                }
                // SetActive(ShowTutorial); a tutorial left active clears the preference.
                "ActivateIfTutorial" if self.prefs.ShowTutorial => self.prefs.ShowTutorial = false,
                // SetActive(Chance > Random.value && FancyEffects): the draw happens whatever the preference.
                // ActivateIfFancyEffects.Awake: the draw and the SetActive are these two arms.
                "ActivateIfFancyEffects" if { let chance = float(v, "Chance").unwrap(); chance > self.random.value() && self.prefs.FancyEffects } => {}
                "ActivateIfTutorial" | "ActivateIfFancyEffects" => self.awake_set_inactive(prefab, inst, linked, &mut enabled_behaviours, o),
                name if super::translated::CLASSES.contains(&name) => {
                    let b = linked.translated[&(o, c)];
                    // The translated code reaches its clone through the instance lists.
                    let root = inst.objects[0].components[0];
                    self.unsynced.push(std::mem::take(inst));
                    self.awaking = true;
                    // Awake and OnEnable are separate messages: an exception in one leaves the other to run, and the
                    // instance goes back whatever happens.
                    let awake = super::menu::catch_managed(|| self.tr_awake(b));
                    let on_enable = (*enabled).then(|| super::menu::catch_managed(|| self.tr_on_enable(b)));
                    self.awaking = false;
                    let at = self.unsynced.iter().rposition(|i| i.objects[0].components[0] == root).expect("the instance being awoken");
                    *inst = self.unsynced.remove(at);
                    // Its SetActive(false) calls, in the Awake pass's own way (the objects' later Awakes skipped).
                    for go in std::mem::take(&mut self.awake_deactivate) {
                        let d = inst.objects.iter().position(|x| x.game_object == go).unwrap_or_else(|| unimplemented!("mod-only: an Awake/OnEnable SetActive(false) of another clone's object"));
                        self.awake_set_inactive(prefab, inst, linked, &mut enabled_behaviours, d);
                    }
                    self.log_exception(awake);
                    self.log_exception(on_enable.flatten());
                }
                // The unit test's Awake that throws.
                #[cfg(test)]
                "ThrowAtAwake" => super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"),
                "LiquidContainerController" => self.liquid_container_awake(linked.liquid_containers[&o]),
                // PostProcessVolume.OnEnable (no Awake).
                "PostProcessVolume" if *enabled => {
                    let id = inst.objects[o].components[c];
                    let v = super::map::post_volume_of(db.as_deref().or(self.assets.as_deref()).expect("the game's data"), prefab, o, c, id);
                    self.post_volume_on_enable(v);
                }
                // No Awake; its OnEnable follows here.
                "ColorWave" if *enabled => match linked.visuals.get(&(o, c)) {
                    Some(&x) => self.visual_on_enable(x),
                    // No SpriteRenderer of its own: OnWillRenderObject never runs, but OnEnable draws its two values.
                    None => self.color_wave_on_enable(),
                },
                "SpriteGradientAnimation" if *enabled => self.visual_on_enable(linked.visuals[&(o, c)]),
                "GateBehaviour" if *enabled => self.gate_on_enable(linked.gates[&o]),
                "RailgunBehaviour" if *enabled => self.railgun_on_enable(linked.railguns[&o]),
                // OnEnable: Joint.useMotor = true (ceiling_turret.rs).
                "CeilingTurretBehaviour" if *enabled => self.ceiling_turret_on_enable(linked.ceiling_turrets[&o]),
                "PointerBehaviour" if *enabled => self.pointer_on_enable(linked.pointers[&o]),
                "ObstructionEmitterBehaviour" if *enabled => self.obstruction_emitter_on_enable(linked.obstruction_emitters[&o]),
                "HeartMonitorBehaviour" if *enabled => self.heart_monitor_update_activation(linked.heart_monitors[&o]),
                "BeamformerBehaviour" if *enabled => self.beamformer_on_enable(linked.beamformers[&o]),
                // OnEnable while the instance is still being built: its colliders come from the instance in hand.
                "TorchBehaviour" if *enabled => {
                    let colliders = Game::torch_child_colliders(inst, o);
                    self.torch_enable_contacts(linked.torches[&o], colliders);
                }
                "DetectorBehaviour" => self.detector_awake(linked.detectors[&o]),
                "SingleFloodlightBehaviour" => self.floodlight_awake(linked.floodlights[&o]),
                "MinigunBehaviour" => self.minigun_awake(linked.miniguns[&o]),
                "BeamformerBehaviour" => self.beamformer_awake(linked.beamformers[&o]),
                c if super::syringe::is_syringe(c) => self.syringe_awake(linked.syringes[&o]),
                // Awake: OnContinuousUpdate.AddListener (KeyTriggerBehaviour's is never removed).
                "KeyTriggerBehaviour" => {
                    self.continuous.listeners.push(super::continuous::Listener::KeyTrigger(linked.key_triggers[&o]));
                    if *enabled {
                        self.key_trigger_on_enable(linked.key_triggers[&o]);
                    }
                }
                "ThermometerBehaviour" if *enabled => self.thermometer_update_activation(linked.thermometers[&o]),
                "DefibrillatorBehaviour" if *enabled => self.defibrillator_on_enable(linked.defibrillators[&o]),
                "HeartMonitorBehaviour" if *enabled => self.heart_monitor_update_activation(linked.heart_monitors[&o]),
                // ActivationTransformerBehaviour.Awake: OnContinuousUpdate.AddListener.
                "ActivationTransformerBehaviour" => self.continuous.listeners.push(super::continuous::Listener::ActivationTransformer(linked.activation_transformers[&o])),
                // ButtonBehaviour.Awake: OnContinuousUpdate.AddListener.
                "ButtonBehaviour" => self.continuous.listeners.push(super::continuous::Listener::Button(linked.buttons[&o])),
                "LaserReceiverBehaviour" => self.continuous.listeners.push(super::continuous::Listener::LaserReceiver(linked.laser_receivers[&o])),
                // LiquidLevelGateBehaviour.Awake, ActivatorElectrodeBehaviour.Awake: OnContinuousUpdate.AddListener (phys = GetComponent is the link).
                "LiquidLevelGateBehaviour" => self.continuous.listeners.push(super::continuous::Listener::LiquidLevelGate(linked.liquid_level_gates[&o])),
                "ActivatorElectrodeBehaviour" => self.continuous.listeners.push(super::continuous::Listener::ActivatorElectrode(linked.activator_electrodes[&o])),
                "FireworksBehaviour" => self.fireworks_awake(linked.fireworks[&o]),
                "TelevisionBehaviour" => self.television_awake(linked.televisions[&o]),
                "EnergySwordBehaviour" => {
                    // Awake: BladeObject.SetActive(false), MotionBlur.SetActive(false).
                    let x = &self.energy_swords[linked.energy_swords[&o]];
                    let (blade, blur) = (x.BladeObject, x.MotionBlur);
                    self.awake_set_inactive(prefab, inst, linked, &mut enabled_behaviours, blade);
                    self.awake_set_inactive(prefab, inst, linked, &mut enabled_behaviours, blur);
                }
                "ImmobilityFieldBehaviour" if *enabled => {
                    // OnEnable: UpdateActivation. Activated shows CaptureObject, which must be active already.
                    let i = linked.immobility_fields[&o];
                    let x = &self.immobility_fields[i];
                    let (on, capture, trigger) = (x.Activated, x.CaptureObject, x.Trigger);
                    match on {
                        true if inst.objects[capture].active => {}
                        true => unimplemented!("mod-only: ImmobilityFieldBehaviour.OnEnable: CaptureObject.SetActive(true) of an inactive object while the clone is awoken"),
                        false => self.awake_set_inactive(prefab, inst, linked, &mut enabled_behaviours, capture),
                    }
                    self.collider_set_enabled(inst, trigger, on);
                    self.immobility_field_destroy_all_joints(i);
                }
                "CardiopulmonaryBypassMachineBehaviour" if *enabled => {
                    // OnEnable: UpdateActivation. Activated shows the Cosmetics object, which the prefab has active already.
                    let x = &self.cardiopulmonary_bypasses[linked.cardiopulmonary_bypasses[&o]];
                    let cosmetics = x.Cosmetics;
                    match x.Activated {
                        true if inst.objects[cosmetics].active => {}
                        true => unimplemented!("mod-only: CardiopulmonaryBypassMachineBehaviour.OnEnable: Cosmetics.SetActive(true) of an inactive object while the clone is awoken"),
                        false => self.awake_set_inactive(prefab, inst, linked, &mut enabled_behaviours, cosmetics),
                    }
                }
                "GateBehaviour" => self.gate_awake(linked.gates[&o]),
                "DangleBehaviour" => {
                    let id = inst.objects[o].components[c];
                    let i = self.dangles.iter().rposition(|x| x.id == id).expect("the linked DangleBehaviour");
                    self.dangle_awake(i);
                }
                "JetEngineBehaviour" => self.jet_engine_awake(linked.jet_engines[&o]),
                // LagboxBehaviour.Awake only looks up the MuteBehaviour.
                "LagboxBehaviour" => {
                    if *enabled {
                        self.lagbox_on_enable(linked.lagboxes[&o]);
                    }
                }
                "TimedGateBehaviour" => {
                    let awake = super::menu::catch_managed(|| self.timed_gate_awake(linked.timed_gates[&o]));
                    self.log_exception(awake);
                    if *enabled {
                        self.timed_gate_on_enable(linked.timed_gates[&o]);
                    }
                }
                "FirearmAttachmentBehaviour" | "ScopeAttachmentBehaviour" | "BatteryAttachmentBehaviour" | "ExplosiveAttachmentBehaviour" | "FlashlightAttachmentBehaviour" | "IncendiaryAttachmentBehaviour" => self.firearm_attachment_awake(),
                "DisplayBehaviour" => self.display_awake(linked.displays[&o]),
                // PhysicsGunBehaviour.Awake: LineRenderer.Initialise(), then its GameObject is hidden.
                "PhysicsGunBehaviour" => {
                    let gun = &self.physics_guns[linked.physics_guns[&o]];
                    let (id, lo) = (gun.line_id, gun.line_object);
                    let s = self.smooth_lines.iter().position(|x| x.id == id).expect("the gun's SmoothLineControllerBehaviour");
                    self.smooth_line_initialise(s);
                    self.awake_set_inactive(prefab, inst, linked, &mut enabled_behaviours, lo);
                }
                // HologramJitter.Awake: offset = Random.Range(-1f, 1f) * 1500f
                "HologramJitter" => {
                    let offset = self.hologram_jitter_offset();
                    let id = inst.objects[o].components[c];
                    if let Some(v) = self.visuals.iter().position(|x| x.id == id) {
                        let super::visual::Kind::Hologram { offset: at, .. } = &mut self.visuals[v].kind else { unreachable!("HologramJitter's visual") };
                        *at = offset;
                    }
                }
                "PersonBehaviour" => {
                    if let Some(s) = self.person_awake(linked.person.unwrap()) {
                        inst.set_local_scale(0, s);
                        self.refresh_transforms(&inst, linked);
                    }
                }
                "SkinMaterialHandler" => self.skin_awake(limb()),
                "PhysicalBehaviour" => self.physical_awake(linked.physicals[&o]),
                "WaterBehaviour" => self.water_awake(linked.waters[&o]),
                "LimbBehaviour" => self.limb_awake(limb()),
                "CirculationBehaviour" => self.circulation_awake(limb()),
                "DamagableMachineryBehaviour" => self.machinery_awake(db.as_deref(), linked.machinery[&o]),
                "FirearmBehaviour" => self.firearm_awake(linked.firearms[&o]),
                "TeslaCoilBehaviour" => self.unity_callback(|g| g.tesla_coil_awake(linked.tesla_coils[&o], &linked.tesla_bolt_sources[&o])),
                "TeslaCoilLightningBoltBehaviour" => {
                    let id = inst.objects[o].components[c];
                    let k = self.tesla_bolts.iter().position(|b| b.id == id).expect("the TeslaCoilLightningBoltBehaviour just linked");
                    self.tesla_bolt_awake(k);
                }
                // RandomSpawnedSize.Awake
                // transform.localScale = Vector2.one * Random.Range(Range.x, Range.y) (a Vector2: z becomes 0).
                "RandomSpawnedSize" => {
                    let range: [f32; 2] = floats(field(v, "Range").unwrap()).unwrap();
                    let s = self.random.range(range[0], range[1]);
                    inst.set_local_scale(o, [s, s, 0.0]);
                    self.refresh_transforms(inst, linked);
                }
                "LocalFirePropagationMap" => self.fire_map_awake(linked.fire_maps[&o]),
                "GearBehaviour" => self.gear_awake(inst.objects[o].components[c]),
                "WeightlessHelper" => self.weightless_helper_awake(inst.objects[o].components[c]),
                "PoolableSizeHelper" => {
                    let scale = [inst.objects[o].trs.s[0], inst.objects[o].trs.s[1], inst.objects[o].trs.s[2]];
                    self.poolable_size_helper_awake(inst.objects[o].components[c], scale);
                }
                "LaunchedRocketBehaviour" => self.rocket_awake(linked.rockets[&o]),
                "BreakJointOnBurnBehaviour" => {
                    let b = self.break_joint_on_burns.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap();
                    self.break_joint_on_burn_awake(b);
                }
                "GenericScifiWeapon40Behaviour" => self.scifi40_awake(linked.scifi40s[&o]),
                "HealthGunBehaviour" | "FreezeGunBehaviour" => self.effect_gun_awake(),
                "AirfoilBehaviour" => self.airfoil_awake(linked.airfoils[&o]),
                "MachineGunProjectileBehaviour" => {
                    let i = self.machine_gun_projectiles.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap();
                    self.machine_gun_projectile_awake(i);
                }
                "WingBehaviour" => {
                    let i = self.wings.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap();
                    self.wing_awake(i);
                }
                "DestroyWhenAllChildrenDestroyed" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let file = prefab.objects[o].game_object.file;
                    let exclude: Vec<usize> = v.get("Exclude").and_then(Value::array).expect("Exclude").iter().filter_map(|g| {
                        let p = g.pptr().filter(|p| !p.is_null() && p.file == 0)?;
                        prefab.index.get(&ObjRef { file, path_id: p.path_id }).map(|&(x, _)| x)
                    }).collect();
                    // A loaded clone's object destroyed before its snapshot (link_recording_item's gone ones, inactive and destroyed) was
                    // there at this Awake as the prefab had it: its ActOnDestroy took an id (the Broken fire extinguisher's Top/trail).
                    let gone_active = |d: usize| self.destroyed_components.contains(&inst.objects[d].components[0]) && self.prefab_object_active(prefab, d);
                    let children: Vec<(usize, bool)> = std::iter::once(o).chain(inst.descendants(o)).filter(|&d| inst.objects[d].active || gone_active(d)).map(|d| (d, exclude.contains(&d))).collect();
                    self.dwacd_awake(inst.objects[o].components[c], inst, &children);
                }
                // FuseBoxBehaviour.ProvideAudioData, ..cctor (d) and ..ctor (rand): antenna.rs.
                "FuseBoxBehaviour" => self.fuse_box_awake(inst.objects[o].components[c]),
                "AntennaReceiverBehaviour" => self.antenna_receiver_awake(linked.antenna_receivers[&o]),
                // PortalBehaviour.Awake: new GameObject("Portal clone renderer") (GameObject, Transform); its position is presentation.
                // PortalBehaviour.OnDestroy: presentation (destroys that object; no instance id is taken).
                // PortalBehaviour.OnWillRenderObject: presentation (calls RenderObjectClones when OtherEnd is set).
                // PortalBehaviour.RenderObjectClones: presentation (pose and scale of the ghost renderers, which nothing reads).
                // PortalBehaviour.OnDrawGizmos: presentation (editor gizmo).
                // PortalTraversingBehaviour.Awake: AddComponent in StartPassage (portal.rs) takes the layers; the component is never in a prefab.
                "PortalBehaviour" => self.allocate_instance_ids(2),
                "SeismicChargeExplosionBehaviour" => self.seismic_explosion_awake(db.as_deref_mut(), inst, o, linked.seismic_explosions[&o]),
                "EMPBehaviour" => self.emp_awake(db.as_deref_mut(), inst, o, linked.emps[&o]),
                "TransmitterBehaviour" => {
                    self.transmitter_awake(linked.transmitters[&o]);
                    if *enabled {
                        self.transmitter_on_enable(linked.transmitters[&o]);
                    }
                }
                "RandomSpriteBehaviour" => self.random_sprite_awake(linked.random_sprites[&o], *enabled),
                "RandomSpriteChildBehaviour" => self.random_sprite_child_awake(linked.random_sprite_children[&o]),
                "BlackHoleBehaviour" => self.black_hole_awake(linked.black_holes[&o]),
                "GorseEyeBehaviour" => {
                    let e = self.gorse_eyes.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap();
                    self.gorse_eye_awake(e);
                }
                "AutomaticWheelJointCreator" => {
                    let wheels: Vec<PPtr> = v.get("Wheels").and_then(Value::array).unwrap().iter().map(|w| w.pptr().unwrap()).collect();
                    self.automatic_wheel_joint_creator_awake(inst, o, &wheels, float(v, "Frequency").unwrap(), float(v, "Dampening").unwrap());
                }
                "CarBehaviour" => self.car_awake(linked.cars[&o], inst, o),
                "WinchBehaviour" => self.winch_awake(linked.winches[&o]),
                "DroneBehaviour" => self.drone_awake(linked.drones[&o]),
                "ReactorConduitBehaviour" => self.reactor_conduit_awake_at(linked.reactor_conduits[&o], prefab, inst, linked, o),
                "JointMotorSoundBehaviour" => self.joint_motor_sound_awake_at(linked.joint_motor_sounds[&o], prefab, inst, linked),
                "IndustrialLiftBehaviour" => self.industrial_lift_awake_at(linked.industrial_lifts[&o], prefab, inst, linked),
                "CopperWireBehaviour" | "NewUseWireBehaviour" => self.copper_wire_awake_at(linked.copper_wires[&o], o, v, prefab, inst, linked),
                "ReactorConduitSetBehaviour" => {
                    let mut objects = Vec::new();
                    super::car::active_below(inst, o, &mut objects);
                    let set = &mut self.reactor_conduit_sets[linked.reactor_conduit_sets[&o]];
                    set.conduit_objects = objects.into_iter().filter(|x| linked.reactor_conduits.contains_key(x)).collect();
                    set.conduits = set.conduit_objects.iter().map(|x| linked.reactor_conduits[x]).collect();
                }
                "TyreDeflationBehaviour" => self.tyre_awake(linked.tyres[&o], inst),
                // Start: Object.Destroy(Joint) for a Broken one, which only a loaded save sets.
                "JointSerialisationHelper" | "BatchJointSerialisationHelper" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let file = prefab.objects[o].game_object.file;
                    let key = |p: PPtr| {
                        if p.is_null() {
                            return None;
                        }
                        assert_eq!(p.file, 0, "{class}: a joint outside its prefab");
                        let k = prefab.index[&ObjRef { file, path_id: p.path_id }];
                        (k.0 == o).then(|| self.break_joint_key(inst, k)).flatten()
                    };
                    let joints: Vec<Option<BreakJoint>> = match class.as_str() {
                        "JointSerialisationHelper" => {
                            // JointSerialisationHelper.Start: Destroy(Joint) of a broken joint is the assert below
                            assert!(!v.get("Broken").and_then(Value::bool).unwrap(), "JointSerialisationHelper.Start: Object.Destroy(Joint) of a broken joint");
                            vec![key(v.get("Joint").and_then(Value::pptr).unwrap())]
                        }
                        _ => v.get("Joints").and_then(Value::array).unwrap().iter().map(|j| key(j.pptr().unwrap())).collect(),
                    };
                    self.joint_helpers.push(super::car::JointHelper { id: inst.objects[o].components[c], batch: class == "BatchJointSerialisationHelper", Broken: vec![false; joints.len()], joints });
                }
                "NoCollideGm" => self.no_collide_gm_awake(inst.objects[o].components[c], inst),
                "RandomCarTextureBehaviour" => {
                    let t = self.random_car_textures.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap();
                    self.random_car_texture_awake(t, *enabled);
                }
                // The statics; the Number is on the object's name, which only the editor shows.
                // EggBehaviour.Awake: canEnter = true and enteredCode = "" (controller, spriteRenderer and audioSource are read by Update and Validate)
                "EggBehaviour" => (self.egg_can_enter, self.egg_entered) = (super::acts::CAN_ENTER, super::acts::ENTERED_CODE.to_string()),
                "PanelsBehaviour" => self.panels_link(prefab, inst, o),
                // WindControllerBehaviour.Awake: windZone = GetComponent<WindZone>().
                "WindControllerBehaviour" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let base_strength = v.get("BaseStrength").and_then(Value::f32).expect("WindControllerBehaviour.BaseStrength");
                    let zone = prefab.objects[o].components.iter().position(|(_, x)| matches!(x, Component::WindZone(_))).map(|z| inst.objects[o].components[z]);
                    self.wind_controllers.push(super::particle_forces::WindController { id: inst.objects[o].components[c], zone, base_strength });
                }
                "OffsetGridBehaviour" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let file = prefab.objects[o].game_object.file;
                    let (to, tc) = prefab.index[&ObjRef { file, path_id: v.get("Blip").and_then(Value::pptr).unwrap().path_id }];
                    let Blip = inst.objects[to].components[tc];
                    self.offset_grids.push(super::acts::OffsetGrid { id: inst.objects[o].components[c], Blip, timer: 0.0 });
                }
                "OnlyExistWith" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let file = prefab.objects[o].game_object.file;
                    let id = |name: &str| {
                        let (to, tc) = prefab.index[&ObjRef { file, path_id: v.get(name).and_then(Value::pptr).unwrap().path_id }];
                        inst.objects[to].components[tc]
                    };
                    let x = super::acts::OnlyExistWith { id: inst.objects[o].components[c], Other: id("Other"), AlsoDelete: id("AlsoDelete") };
                    self.only_exist_withs.push(x);
                }
                // Awake subscribes Event to PhysicalBehaviour.OnDisintegration.
                "ActOnDisintegrate" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let file = prefab.objects[o].game_object.file;
                    let physical = v.get("PhysicalBehaviour").and_then(Value::pptr).unwrap();
                    let (to, _) = prefab.index[&ObjRef { file, path_id: physical.path_id }];
                    let calls = event_calls(prefab, file, v.get("Event").unwrap(), inst, "ActOnDisintegrate.Event", false).unwrap();
                    self.act_on_disintegrates.push(super::acts::ActOnDisintegrate { id: inst.objects[o].components[c], physical: linked.physicals[&to], enabled: *enabled, calls });
                }
                // Registered with its object: OnJointBreak2D for the joints on its own GameObject (SendMessage reaches no
                // other) and the break limit key each sits under.
                "JointBreakSoundBehaviour" => {
                    // JointBreakSoundBehaviour.Start: empty
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let file = prefab.objects[o].game_object.file;
                    let local = |p: PPtr| {
                        assert_eq!(p.file, 0, "JointBreakSoundBehaviour: a reference outside its prefab");
                        (!p.is_null()).then(|| prefab.index[&ObjRef { file, path_id: p.path_id }])
                    };
                    let joints = v.get("Joints").and_then(Value::array).unwrap().iter().filter_map(|j| local(j.pptr().unwrap())).filter(|&(jo, _)| jo == o);
                    let joints = joints.filter_map(|k| self.break_joint_key(inst, k)).collect();
                    // OnBreak: ExplosiveBehaviour.ForceImmediateExplosion, DamagableMachineryBehaviour.BreakPermanently, audio and particles.
                    let mut explosives = Vec::new();
                    let mut calls = Vec::new();
                    for call in invoked_calls(v.get("OnBreak").unwrap()) {
                        if let Some(a) = audio_call(prefab, file, inst, call) {
                            calls.push(a);
                            continue;
                        }
                        // A GameObject target (no component): SetActive.
                        let target = call.get("m_Target").and_then(Value::pptr).unwrap();
                        let go = (target.file == 0 && string(call, "m_MethodName").is_ok_and(|m| m == "SetActive"))
                            .then(|| prefab.index.get(&ObjRef { file, path_id: target.path_id }).copied())
                            .flatten()
                            .filter(|&(_, c)| c == usize::MAX);
                        if let Some((to, _)) = go {
                            let on = call.get("m_Arguments").and_then(|a| a.get("m_BoolArgument")).and_then(|b| b.bool().or(b.i64().map(|i| i != 0))).expect("JointBreakSoundBehaviour.OnBreak: SetActive without a bool");
                            let physics = |x: usize| prefab.objects[x].components.iter().any(|(_, k)| matches!(k, Component::Rigidbody2D(_) | Component::Collider2D(_) | Component::Joint2D(_)));
                            if physics(to) || inst.descendants(to).into_iter().any(physics) {
                                unimplemented!("mod-only: JointBreakSoundBehaviour.OnBreak: SetActive of a subtree with physics");
                            }
                            calls.push(EventCall::SetActive(inst.objects[to].components[0], on));
                            continue;
                        }
                        match call_target(prefab, file, call) {
                            Some((to, tc)) if matches!(&prefab.objects[to].components[tc].1, Component::Script { class, .. } if class == "ExplosiveBehaviour") && string(call, "m_MethodName").unwrap() == "ForceImmediateExplosion" => {
                                explosives.push(linked.explosives[&to]);
                            }
                            _ if presentation_call(prefab, file, call) => {}
                            Some((to, tc)) if matches!(&prefab.objects[to].components[tc].1, Component::Script { class, .. } if class == "CarBehaviour") => {}
                            Some((to, tc)) if matches!(&prefab.objects[to].components[tc].1, Component::Script { class, .. } if class == "AddForceBehaviour") && string(call, "m_MethodName").unwrap() == "AddRelativeForce" => {
                                let intensity = call.get("m_Arguments").and_then(|a| a.get("m_FloatArgument")).and_then(Value::f32).unwrap();
                                calls.push(EventCall::AddRelativeForce(inst.objects[to].components[tc], intensity));
                            }
                            Some((to, tc)) if matches!(&prefab.objects[to].components[tc].1, Component::Script { class, .. } if class == "DamagableMachineryBehaviour") && string(call, "m_MethodName").unwrap() == "BreakPermanently" => {
                                calls.push(EventCall::MachineryBreakPermanently(inst.objects[to].components[tc]));
                            }
                            Some((to, tc)) if matches!(&prefab.objects[to].components[tc].1, Component::Script { class, .. } if class == "InstantiateObjectBehaviour") && string(call, "m_MethodName").is_ok_and(|m| m == "Instantiate") => {
                                calls.push(EventCall::Instantiate(inst.objects[to].components[tc]));
                            }
                            Some((to, tc)) if matches!(&prefab.objects[to].components[tc].1, Component::Script { class, .. } if class == "TankBarrelBehaviour") && string(call, "m_MethodName").is_ok_and(|m| m == "set_enabled") => {
                                calls.push(EventCall::DisableTankBarrel(inst.objects[to].components[tc]));
                            }
                            // The other persistent calls take the shared event parser (one call at a time).
                            Some(_) => {
                                let one = Value::Struct(vec![("m_PersistentCalls".into(), Value::Struct(vec![("m_Calls".into(), Value::Array(vec![call.clone()]))]))]);
                                match try_event_calls(prefab, file, &one, inst, false).unwrap() {
                                    Ok(mut kept) => calls.append(&mut kept),
                                    Err(_) => calls.push(EventCall::Stop(super::unity_event::stop_for(prefab, file, call, "JointBreakSoundBehaviour.OnBreak"))),
                                }
                            }
                            _ => calls.extend(super::unity_event::call(prefab, file, inst, call, "JointBreakSoundBehaviour.OnBreak").unwrap()),
                        }
                    }
                    let cars = car_calls(prefab, file, v.get("OnBreak").unwrap(), inst);
                    self.joint_break_sounds.push(super::misc::JointBreakSound {
                        id: inst.objects[o].components[c],
                        joints,
                        explosives,
                        cars,
                        calls,
                        enabled: *enabled,
                        clips: v.get("Clips").and_then(Value::array).map_or(0, <[_]>::len),
                        physical: linked.physicals.get(&o).copied(),
                    });
                }
                _ => {}
            });
            self.log_exception(caught);
            // Behaviour::AddToManager after Awake, for an enabled one.
            let behaviour = match class.as_str() {
                "PersonBehaviour" => Some(Behaviour::Person(linked.person.unwrap())),
                "CirculationBehaviour" => Some(Behaviour::Circulation(limb())),
                "SkinMaterialHandler" => Some(Behaviour::Skin(limb())),
                "GripBehaviour" => Some(Behaviour::Grip(self.limbs[limb()].GripBehaviour.unwrap())),
                "GoreStringBehaviour" => Some(Behaviour::GoreString(limb())),
                "GForceMeasureBehaviour" => Some(Behaviour::GForce(limb())),
                "DamagableMachineryBehaviour" => Some(Behaviour::Machinery(linked.machinery[&o])),
                "LavaBehaviour" => Some(Behaviour::Lava(linked.lavas[&o])),
                "RandomPrefabSpawnerBehaviour" => Some(Behaviour::Spawner(linked.spawners[&o])),
                "TemperatureTargetRectangleBehaviour" => Some(Behaviour::HeatRect(linked.heat_rects[&o])),
                "DestroyableBehaviour" | "VaseBehaviour" => Some(Behaviour::Destroyable(linked.destroyables[&o])),
                "LocalFirePropagationMap" => Some(Behaviour::FireMap(linked.fire_maps[&o])),
                "FirearmBehaviour" => Some(Behaviour::Firearm(linked.firearms[&o])),
                "ProjectileLauncherBehaviour" => Some(Behaviour::Launcher(linked.launchers[&o])),
                "BellBehaviour" => Some(Behaviour::Bell(linked.bells[&o])),
                "MachineGunBehaviour" => Some(Behaviour::MachineGun(linked.machine_guns[&o])),
                "MinigunBehaviour" => Some(Behaviour::Minigun(linked.miniguns[&o])),
                "AcceleratorGunBehaviour" => Some(Behaviour::Accelerator(linked.accelerators[&o])),
                "AcceleratorBoltBehaviour" => Some(Behaviour::AcceleratorBolt(linked.accelerator_bolts[&o])),
                "ArchelixCasterBehaviour" => Some(Behaviour::ArchelixCaster(linked.archelix_casters[&o])),
                "ArchelixCasterBoltBehaviour" => Some(Behaviour::ArchelixBolt(linked.archelix_bolts[&o])),
                "UniversalBellBehaviour" => Some(Behaviour::UniversalBell(linked.universal_bells[&o])),
                "CounterBehaviour" => Some(Behaviour::Counter(linked.counters[&o])),
                "PointerBehaviour" => Some(Behaviour::Pointer(linked.pointers[&o])),
                "ToggleableMirrorBehaviour" => Some(Behaviour::ToggleableMirror(linked.toggleable_mirrors[&o])),
                "ValveBehaviour" | "LiquidPressureValveBehaviour" => Some(Behaviour::Valve(linked.valves[&o])),
                "LiquidCanisterBehaviour" => Some(Behaviour::LiquidCanister(linked.liquid_canisters[&o])),
                "LiquidFilterBehaviour" => Some(Behaviour::LiquidFilter(linked.liquid_filters[&o])),
                "PulseDrumBehaviour" => Some(Behaviour::PulseDrum(linked.pulse_drums[&o])),
                "LiquidentifierBehaviour" => Some(Behaviour::Liquidentifier(linked.liquidentifiers[&o])),
                "CentrifugeBehaviour" => Some(Behaviour::Centrifuge(linked.centrifuges[&o])),
                "VibrationBehaviour" => Some(Behaviour::Vibration(linked.vibrations[&o])),
                "OnlyExistWith" => Some(Behaviour::OnlyExistWith(self.only_exist_withs.len() - 1)),
                "OffsetGridBehaviour" => Some(Behaviour::OffsetGrid(self.offset_grids.len() - 1)),
                "WindControllerBehaviour" => Some(Behaviour::WindController(self.wind_controllers.len() - 1)),
                "ParticleMachineBehaviour" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    self.particle_machines.push(v.get("Activated").and_then(Value::bool).expect("ParticleMachineBehaviour.Activated"));
                    Some(Behaviour::ParticleMachine(self.particle_machines.len() - 1))
                }
                "EggBehaviour" => {
                    let id = inst.objects[o].components[c];
                    Some(Behaviour::Egg(self.eggs.iter().rposition(|e| e.id == id).unwrap()))
                }
                "LiquidPumpBehaviour" => Some(Behaviour::LiquidPump(linked.liquid_pumps[&o])),
                "LiquidDuplicatorBehaviour" => Some(Behaviour::LiquidDuplicator(linked.liquid_duplicators[&o])),
                "LiquidContainerHoleBehaviour" => Some(Behaviour::LiquidHole(linked.liquid_holes[&o])),
                "TimedGateBehaviour" => Some(Behaviour::TimedGate(linked.timed_gates[&o])),
                "LagboxBehaviour" => Some(Behaviour::Lagbox(linked.lagboxes[&o])),
                "GateBehaviour" => Some(Behaviour::Gate(linked.gates[&o])),
                "LiquidLevelGateBehaviour" => Some(Behaviour::LiquidLevelGate(linked.liquid_level_gates[&o])),
                "ActivationTransformerBehaviour" => Some(Behaviour::ActivationTransformer(linked.activation_transformers[&o])),
                "HoverThrusterBehaviour" => Some(Behaviour::HoverThruster(linked.hover_thrusters[&o])),
                "HovercarBehaviour" => Some(Behaviour::Hovercar(linked.hovercars[&o])),
                "WinchBehaviour" => Some(Behaviour::Winch(linked.winches[&o])),
                "ConveyorBeltBehaviour" => Some(Behaviour::Conveyor(linked.conveyors[&o])),
                "ReactorCoreBehaviour" => Some(Behaviour::ReactorCore(linked.reactor_cores[&o])),
                "ReactorA5DisplayBehaviour" => Some(Behaviour::ReactorA5Display(linked.reactor_a5_displays[&o])),
                "ReactorConduitBehaviour" => Some(Behaviour::ReactorConduit(linked.reactor_conduits[&o])),
                "ReactorMeltdownBehaviour" => Some(Behaviour::ReactorMeltdown(linked.reactor_meltdowns[&o])),
                "DoAtTime" => Some(Behaviour::DoAtTime(linked.do_at_times[&o])),
                "IndustrialLiftBehaviour" => Some(Behaviour::IndustrialLift(linked.industrial_lifts[&o])),
                "CopperWireBehaviour" | "NewUseWireBehaviour" => Some(Behaviour::BloodWire(linked.copper_wires[&o])),
                "ep" => Some(Behaviour::Ep(linked.eps[&o])),
                "AcidPoolBehaviour" => Some(Behaviour::AcidPool(linked.acid_pools[&o])),
                "VacuumComponent" => Some(Behaviour::Vacuum(linked.vacuums[&o])),
                "PointAtBehaviour" => Some(Behaviour::PointAt(linked.point_ats[&o])),
                "JukeboxBehaviour" => Some(Behaviour::Jukebox(linked.jukeboxes[&o])),
                "AntennaReceiverBehaviour" => Some(Behaviour::AntennaReceiver(linked.antenna_receivers[&o])),
                "AdhesiveCouplerBehaviour" => Some(Behaviour::Coupler(linked.couplers[&o])),
                "LightningToolEntityBehaviour" => Some(Behaviour::LightningEntity(linked.lightning_entities[&o])),
                "PyroPowerToolEntity" => Some(Behaviour::PyroEntity(linked.pyro_entities[&o])),
                "SeismicChargeExplosionBehaviour" => Some(Behaviour::SeismicExplosion(linked.seismic_explosions[&o])),
                "EnergyVesselLightningBehaviour" => Some(Behaviour::EnergyBolt(linked.energy_bolts[&o])),
                "TransmitterBehaviour" => Some(Behaviour::Transmitter(linked.transmitters[&o])),
                "DefibrillatorBehaviour" => Some(Behaviour::Defibrillator(linked.defibrillators[&o])),
                "CardiopulmonaryBypassMachineBehaviour" => Some(Behaviour::CardiopulmonaryBypass(linked.cardiopulmonary_bypasses[&o])),
                "HeartMonitorBehaviour" => Some(Behaviour::HeartMonitor(linked.heart_monitors[&o])),
                "PowerGaugeBehaviour" | "PressureGaugeBehaviour" | "TemperatureGaugeBehaviour" => Some(Behaviour::PowerGauge(linked.power_gauges[&o])),
                "ExcaliburBehaviour" => Some(Behaviour::Excalibur(linked.excaliburs[&o])),
                "SharpOnAllSidesBehaviour" => Some(Behaviour::SharpOnAllSides(linked.sharp_on_all_sides[&o])),
                "BloodTankBehaviour" => Some(Behaviour::BloodTank(linked.blood_tanks[&o])),
                "LiquidOutletBehaviour" => Some(Behaviour::LiquidOutlet(linked.liquid_outlets[&o])),
                "ExplosiveBehaviour" => Some(Behaviour::Explosive(linked.explosives[&o])),
                "DeleteAfterTime" => Some(Behaviour::DeleteAfter(linked.delete_afters[&o])),
                "InfiniteLiquidSourceBehaviour" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    // Awake: c = GetComponent<BloodContainer>() (RequireComponent).
                    let owner = if let Some(&f) = linked.flasks.get(&o) {
                        super::container::Owner::Flask(f)
                    } else if let Some(&s) = linked.syringes.get(&o) {
                        super::container::Owner::Syringe(s)
                    } else {
                        unimplemented!("mod-only: InfiniteLiquidSourceBehaviour on a BloodContainer other than a flask or syringe")
                    };
                    self.infinite_liquids.push((string(v, "LiquidID").expect("InfiniteLiquidSourceBehaviour.LiquidID"), owner));
                    Some(Behaviour::InfiniteLiquid(self.infinite_liquids.len() - 1))
                }
                "SmoothLineControllerBehaviour" => Some(Behaviour::SmoothLine(self.smooth_lines.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "ScherbewerferBehaviour" => Some(Behaviour::Scherbewerfer(self.scherbewerfers.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "NeedlerBehaviour" => Some(Behaviour::Needler(self.needlers.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "NeedleBehaviour" => Some(Behaviour::Needle(self.needles.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "IgnoreParentSize" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let size = field(v, "DesiredSize").expect("IgnoreParentSize.DesiredSize");
                    let DesiredSize = [float(size, "x").unwrap(), float(size, "y").unwrap()];
                    self.ignore_parent_sizes.push(super::explosion::IgnoreParentSize { id: inst.objects[o].components[c], DesiredSize, oldParent: None, started: false, waiting: false });
                    Some(Behaviour::IgnoreParentSize(self.ignore_parent_sizes.len() - 1))
                }
                "WhOpening" => Some(Behaviour::WhOpening(self.wh_openings.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "wh" => Some(Behaviour::Wh(self.whs.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "RaygunBehaviour" => Some(Behaviour::Raygun(self.raygun_beams.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "MachineGunProjectileBehaviour" => Some(Behaviour::MachineGunProjectile(self.machine_gun_projectiles.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "PlaneBehaviour" => Some(Behaviour::Plane(self.planes.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "WingBehaviour" => Some(Behaviour::Wing(self.wings.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "ShockwaveBehaviour" => Some(Behaviour::Shockwave(self.shockwaves.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "GlassShardBehaviour" => Some(Behaviour::GlassShard(self.glass_shards.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "CustomFixedConstrainedBehaviour" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    // A Rigidbody2D of this prefab by its object (null: None; another file: not in a spawned prefab).
                    let owner_of = |g: &Game, obj: usize| inst.bodies.get(&obj).and_then(|&b| g.physicals.iter().position(|p| p.rigidbody.body == b).map(BodyOwner::Physical).or_else(|| g.node_rigidbodies.iter().position(|n| n.rigidbody.body == b).map(BodyOwner::Node)));
                    let other = v.get("Other").and_then(Value::pptr).filter(|p| !p.is_null()).map(|p| {
                        let (to, _) = prefab.index.iter().find(|(k, _)| k.path_id == p.path_id).map(|(_, v)| *v).expect("CustomFixedConstrainedBehaviour.Other outside the prefab");
                        owner_of(self, to).expect("CustomFixedConstrainedBehaviour.Other: a Rigidbody2D the arenas hold")
                    });
                    let vec2 = |name: &str| floats::<2>(field(v, name).unwrap()).unwrap();
                    let physical = linked.physicals.get(&o).copied().unwrap_or_else(|| unimplemented!("mod-only: CustomFixedConstrainedBehaviour on an object without a PhysicalBehaviour"));
                    // CustomFixedConstrainedBehaviour.Awake: rb = GetComponent<Rigidbody2D>().
                    let rb = owner_of(self, o);
                    self.custom_fixeds.push(super::custom_fixed::CustomFixedConstrained {
                        id: inst.objects[o].components[c],
                        physical,
                        Other: other,
                        LocalPosition: vec2("LocalPosition"),
                        RelativeAngle: v.get("RelativeAngle").and_then(Value::f32).unwrap_or(0.0),
                        AutoAnchor: v.get("AutoAnchor").and_then(Value::bool).unwrap_or(true),
                        rb,
                    });
                    Some(Behaviour::CustomFixedConstrained(self.custom_fixeds.len() - 1))
                }
                "SlightMovement" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let mut s = super::slight_movement::SlightMovement::new(inst.objects[o].components[c]);
                    s.Intensity = v.get("Intensity").and_then(Value::f32).unwrap_or(s.Intensity);
                    s.Speed = v.get("Speed").and_then(Value::f32).unwrap_or(s.Speed);
                    self.slight_movements.push(s);
                    Some(Behaviour::SlightMovement(self.slight_movements.len() - 1))
                }
                "PressureGridController" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let mut s = super::grid::PressureGrid::new(inst.objects[o].components[c]);
                    s.Width = v.get("Width").and_then(Value::i64).map_or(s.Width, |x| x as i32);
                    s.Height = v.get("Height").and_then(Value::i64).map_or(s.Height, |x| x as i32);
                    s.GridScale = v.get("GridScale").and_then(Value::f32).unwrap_or(s.GridScale);
                    s.Retention = v.get("Retention").and_then(Value::f32).unwrap_or(s.Retention);
                    s.BalanceForce = v.get("BalanceForce").and_then(Value::f32).unwrap_or(s.BalanceForce);
                    s.BrushDistance = v.get("BrushDistance").and_then(Value::f32).unwrap_or(s.BrushDistance);
                    // Awake: Grid = new Grid<PressureCell>(Width, Height), a fresh PressureCell in each slot.
                    s.make_grid();
                    self.pressure_grids.push(s);
                    Some(Behaviour::PressureGrid(self.pressure_grids.len() - 1))
                }
                "DisableWhenControlTrigger" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let file = prefab.objects[o].game_object.file;
                    let next = v.get("NextStep").and_then(Value::pptr).filter(|p| p.path_id != 0).map(|p| {
                        let (to, tc) = prefab.index[&ObjRef { file, path_id: p.path_id }];
                        inst.objects[to].components[tc]
                    });
                    self.disable_triggers.push(super::disable_trigger::DisableWhenControlTrigger {
                        id: inst.objects[o].components[c],
                        name: prefab.objects[o].name.clone(),
                        Control: string(v, "Control").unwrap_or_default(),
                        IdentityOverride: string(v, "IdentityOverride").unwrap_or_default(),
                        NextStep: next,
                        NeverSkip: v.get("NeverSkip").and_then(Value::bool).unwrap_or(false),
                    });
                    Some(Behaviour::DisableTrigger(self.disable_triggers.len() - 1))
                }
                "RadiationGridController" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let mut s = super::grid::RadiationGrid::new(inst.objects[o].components[c]);
                    s.GridSize = v.get("GridSize").and_then(Value::f32).unwrap_or(s.GridSize);
                    s.VelocityRetainment = v.get("VelocityRetainment").and_then(Value::f32).unwrap_or(s.VelocityRetainment);
                    s.ValueDropRate = v.get("ValueDropRate").and_then(Value::f32).unwrap_or(s.ValueDropRate);
                    s.ValueTransferRate = v.get("ValueTransferRate").and_then(Value::f32).unwrap_or(s.ValueTransferRate);
                    s.start();
                    self.radiation_grids.push(s);
                    Some(Behaviour::RadiationGrid(self.radiation_grids.len() - 1))
                }
                "ReturnToPoolAfterTime" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let life = v.get("Life").and_then(Value::f32).expect("ReturnToPoolAfterTime.Life");
                    self.return_to_pools.push(super::pool::ReturnToPoolAfterTime { id: inst.objects[o].components[c], Life: life, time: 0.0, pool: None });
                    Some(Behaviour::ReturnToPool(self.return_to_pools.len() - 1))
                }
                "PointToVelocityBehaviour" => Some(Behaviour::PointToVelocity(linked.point_to_velocities[&o])),
                "InverterBehaviour" => Some(Behaviour::Inverter(linked.inverters[&o])),
                "ObstructionEmitterBehaviour" => Some(Behaviour::ObstructionEmitter(linked.obstruction_emitters[&o])),
                "AndroidSparkCreator" => Some(Behaviour::AndroidSpark(linked.android_sparks[&o])),
                "CarBehaviour" => Some(Behaviour::Car(linked.cars[&o])),
                "TyreDeflationBehaviour" => Some(Behaviour::Tyre(linked.tyres[&o])),
                "GasTankBehaviour" | "FlaskBehaviour" => Some(Behaviour::Flask(linked.flasks[&o])),
                "LiquidContainerController" => Some(Behaviour::LiquidContainer(linked.liquid_containers[&o])),
                "TruckDoorBehaviour" => Some(Behaviour::TruckDoor(linked.truck_doors[&o])),
                "GlowtubeBehaviour" => Some(Behaviour::Glowtube(linked.glowtubes[&o])),
                "CathodeLightBehaviour" => Some(Behaviour::CathodeLight(linked.cathode_lights[&o])),
                "LEDBulbBehaviour" => Some(Behaviour::Led(linked.leds[&o])),
                "DetectorBehaviour" => Some(Behaviour::Detector(linked.detectors[&o])),
                "StickyGrenadeBehaviour" => Some(Behaviour::Sticky(linked.sticky_grenades[&o])),
                "GeneratorBehaviour" => Some(Behaviour::Generator(linked.generators[&o])),
                "SingleFloodlightBehaviour" => Some(Behaviour::Floodlight(linked.floodlights[&o])),
                "SpikeGrenadeBehaviour" => Some(Behaviour::SpikeGrenade(linked.spike_grenades[&o])),
                "HeatingElementBehaviour" => Some(Behaviour::HeatingElement(linked.heating_elements[&o])),
                "TemperatureRayGunBehaviour" => Some(Behaviour::RayGun(linked.ray_guns[&o])),
                "BulbBehaviour" => Some(Behaviour::Bulb(linked.bulbs[&o])),
                "ThrusterbedBehaviour" => Some(Behaviour::Thrusterbed(linked.thrusterbeds[&o])),
                "PistonBehaviour" => Some(Behaviour::Piston(linked.pistons[&o])),
                "MapLiftBehaviour" => Some(Behaviour::MapLift(linked.map_lifts[&o])),
                "JointMotorSoundBehaviour" => Some(Behaviour::JointMotorSound(linked.joint_motor_sounds[&o])),
                "FlamethrowerBehaviour" => Some(Behaviour::Flamethrower(linked.flamethrowers[&o])),
                "LaunchedRocketBehaviour" => Some(Behaviour::Rocket(linked.rockets[&o])),
                "FancyBloodSplatController" => Some(Behaviour::Splat(self.splats.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "GorseEyeBehaviour" => Some(Behaviour::GorseEye(self.gorse_eyes.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "GorseDroolingBehaviour" => Some(Behaviour::GorseDrooling(self.gorse_droolings.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "ParticleSystemSerialisationHelper" => Some(Behaviour::SerialisationHelper(self.serialisation_helpers.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "EmitParticlesOnShakeBehaviour" => Some(Behaviour::EmitOnShake(self.emit_on_shakes.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "SoundOnShakeBehaviour" => Some(Behaviour::SoundOnShake(self.sound_on_shakes.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "BreakJointOnBurnBehaviour" => Some(Behaviour::BreakJointOnBurn(self.break_joint_on_burns.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "AirfoilBehaviour" => Some(Behaviour::Airfoil(linked.airfoils[&o])),
                "StunnerBehaviour" => Some(Behaviour::Stunner(linked.stunners[&o])),
                "BalloonBehaviour" => Some(Behaviour::Balloon(linked.balloons[&o])),
                "TorchBehaviour" => Some(Behaviour::Torch(linked.torches[&o])),
                "FlareBehaviour" => Some(Behaviour::Flare(linked.flares[&o])),
                "ButtonBehaviour" => Some(Behaviour::Button(linked.buttons[&o])),
                "DisplayBehaviour" => Some(Behaviour::Display(linked.displays[&o])),
                "BatteryAttachmentBehaviour" => Some(Behaviour::BatteryAttachment(linked.firearm_attachments[&o])),
                "TelevisionBehaviour" => Some(Behaviour::Television(linked.televisions[&o])),
                "ScopeAttachmentBehaviour" | "ExplosiveAttachmentBehaviour" | "FlashlightAttachmentBehaviour" | "IncendiaryAttachmentBehaviour" => Some(Behaviour::FirearmAttachment(linked.firearm_attachments[&o])),
                "LaserBehaviour" => Some(Behaviour::Laser(linked.lasers[&o])),
                "MetronomeBehaviour" => Some(Behaviour::Metronome(linked.metronomes[&o])),
                "DeterminedAnomaly" => Some(Behaviour::DeterminedAnomaly(linked.determined_anomalies[&o])),
                "PhysicsGunBehaviour" => Some(Behaviour::PhysicsGun(linked.physics_guns[&o])),
                "ActivatorElectrodeBehaviour" => Some(Behaviour::ActivatorElectrode(linked.activator_electrodes[&o])),
                "ThermometerBehaviour" => Some(Behaviour::Thermometer(linked.thermometers[&o])),
                "InfraredThermometer" => Some(Behaviour::InfraredThermometer(linked.infrared_thermometers[&o])),
                "JetEngineBehaviour" => Some(Behaviour::JetEngine(linked.jet_engines[&o])),
                "ChainsawBehaviour" => Some(Behaviour::Chainsaw(linked.chainsaws[&o])),
                "CeilingTurretBehaviour" => Some(Behaviour::CeilingTurret(linked.ceiling_turrets[&o])),
                "TemperatureTransferBehaviour" => Some(Behaviour::TemperatureTransfer(linked.temperature_transfers[&o])),
                "WormStaffBehaviour" => Some(Behaviour::WormStaff(linked.worm_staffs[&o])),
                "NewAtomBombExplosionBehaviour" => Some(Behaviour::AtomExplosion(linked.atom_explosions[&o])),
                "AtomBombBehaviour" => Some(Behaviour::AtomBomb(linked.prefab_bombs[&o])),
                "SeismicChargeBehaviour" => Some(Behaviour::SeismicCharge(linked.prefab_bombs[&o])),
                "LaserTurretBehaviour" => Some(Behaviour::LaserTurret(linked.laser_turrets[&o])),
                "SentryTurretBehaviour" => Some(Behaviour::SentryTurret(linked.sentry_turrets[&o])),
                "SentryTurretGunBehaviour" => Some(Behaviour::SentryTurretGun(linked.sentry_guns[&o])),
                "IonBoltBehaviour" => Some(Behaviour::IonBolt(linked.ion_bolts[&o])),
                "RandomEventBehaviour" => Some(Behaviour::RandomEvent(self.random_events.iter().position(|x| x.id == inst.objects[o].components[c]).expect("a linked RandomEventBehaviour"))),
                "EMPBehaviour" => Some(Behaviour::Emp(linked.emps[&o])),
                "DecimatorBehaviour" => Some(Behaviour::Decimator(linked.decimators[&o])),
                "DroneBehaviour" => Some(Behaviour::Drone(linked.drones[&o])),
                "BlackHoleBehaviour" => Some(Behaviour::BlackHole(linked.black_holes[&o])),
                "ImmobilityFieldBehaviour" => Some(Behaviour::ImmobilityField(linked.immobility_fields[&o])),
                "ForcefieldGeneratorBehaviour" => Some(Behaviour::Forcefield(self.forcefields.iter().position(|f| f.id == inst.objects[o].components[c]).expect("the ForcefieldGeneratorBehaviour just linked"))),
                "LightningGunBehaviour" => Some(Behaviour::LightningGun(linked.lightning_guns[&o])),
                "JavelinMissileBehaviour" => Some(Behaviour::JavelinMissile(linked.javelin_missiles[&o])),
                "JavelinBehaviour" => Some(Behaviour::Javelin(linked.javelins[&o])),
                "TeslaCoilBehaviour" => Some(Behaviour::TeslaCoil(linked.tesla_coils[&o])),
                "TeslaCoilLightningBoltBehaviour" => Some(Behaviour::TeslaBolt(self.tesla_bolts.iter().position(|b| b.id == inst.objects[o].components[c]).expect("the TeslaCoilLightningBoltBehaviour just linked"))),
                "EnergySwordBehaviour" => Some(Behaviour::EnergySword(linked.energy_swords[&o])),
                "EnergySwordMotionBlur" => Some(Behaviour::EnergySwordBlur(self.energy_blurs.iter().position(|b| b.id == inst.objects[o].components[c]).expect("the EnergySwordMotionBlur just linked"))),
                "BlasterBehaviour" => Some(Behaviour::Blaster(linked.blasters[&o])),
                "BeamCannonBehaviour" => Some(Behaviour::BeamCannon(linked.beam_cannons[&o])),
                "BeamRepeaterBehaviour" => Some(Behaviour::BeamRepeater(linked.beam_repeaters[&o])),
                "AndroidLaserBehaviour" => Some(Behaviour::AndroidLaser(linked.android_lasers[&o])),
                "RayCannonBehaviour" => Some(Behaviour::RayCannon(linked.ray_cannons[&o])),
                "RailgunBehaviour" => Some(Behaviour::Railgun(linked.railguns[&o])),
                "LaserReceiverBehaviour" => Some(Behaviour::LaserReceiver(linked.laser_receivers[&o])),
                "KeyTriggerBehaviour" => Some(Behaviour::KeyTrigger(linked.key_triggers[&o])),
                "FireworksBehaviour" => Some(Behaviour::Fireworks(linked.fireworks[&o])),
                "BlasterboltBehaviour" => Some(Behaviour::Blasterbolt(linked.blasterbolts[&o])),
                "BeamformerBehaviour" => Some(Behaviour::Beamformer(linked.beamformers[&o])),
                "GenericScifiWeapon40Behaviour" => Some(Behaviour::Scifi40(linked.scifi40s[&o])),
                "HealthGunBehaviour" => Some(Behaviour::HealthGun(linked.effect_guns[&o])),
                "FreezeGunBehaviour" => Some(Behaviour::FreezeGun(linked.effect_guns[&o])),
                "SignalConverterBehaviour" => Some(Behaviour::SignalConverter(linked.signal_converters[&o])),
                c if super::syringe::is_syringe(c) => Some(Behaviour::Syringe(linked.syringes[&o])),
                "PointLiquidTransferBehaviour" => Some(Behaviour::PointLiquid(linked.point_liquids[&o])),
                name if super::translated::CLASSES.contains(&name) => Some(Behaviour::Translated(linked.translated[&(o, c)])),
                // 1.28.45 (v45_new.rs): the link and Awake of its new classes.
                name if super::v45_new::LINKED.contains(&name) && crate::assets::il::is_1_28_45() => self.v45_link(prefab, inst, linked, o, c),
                "TrivialAnimationActivationBehaviour" => Some(Behaviour::Visual("TrivialAnimationActivationBehaviour", linked.visuals[&(o, c)])),
                "SpriteGradientAnimation" => Some(Behaviour::Visual("SpriteGradientAnimation", linked.visuals[&(o, c)])),
                "VertexColourBasedOnTemperatureBehaviour" => Some(Behaviour::Visual("VertexColourBasedOnTemperatureBehaviour", linked.visuals[&(o, c)])),
                _ => None,
            };
            if let Some(b) = behaviour {
                self.behaviour_of.insert(inst.objects[o].components[c], (*execution_order, b));
                if *enabled {
                    self.behaviours.add(*execution_order, b);
                    // ParticleMachineBehaviour.OnEnable: UpdateActivation.
                    if let Behaviour::ParticleMachine(m) = b {
                        self.particle_machine_update_activation(m, false);
                    }
                }
            }
        }
        for (id, q) in std::mem::take(&mut self.text_rotation_pending) {
            match inst.objects.iter().position(|x| x.components.contains(&id)) {
                Some(o) => inst.set_local_rotation(o, q),
                None => self.text_rotation_pending.push((id, q)),
            }
        }
    }

    /// ReactorConduitBehaviour.Awake: Broken = false; FindObjectOfType<ReactorCoreBehaviour>(); the first FixedJoint2D and
    /// HingeJoint2D on the object; with a tube joint its break force, Random.Range(0, 100), no charge particles and the
    /// connected body's PhysicalBehaviour; then the push target's source.
    pub(crate) fn reactor_conduit_awake_at(&mut self, c: usize, prefab: &Prefab, inst: &Instance, linked: &Linked, o: usize) {
        self.reactor_conduits[c].Broken = false;
        let core = (0..self.reactor_cores.len()).find(|&k| !self.destroyed_components.contains(&self.reactor_cores[k].id));
        self.reactor_conduits[c].core = core;
        let joint = |fixed: bool| {
            prefab.objects[o].components.iter().enumerate().find_map(|(k, (_, comp))| match comp {
                Component::Joint2D(j) if matches!(j.kind, JointKind::Fixed { .. }) == fixed && (fixed || matches!(j.kind, JointKind::Hinge { .. })) => Some((k, j.clone())),
                _ => None,
            })
        };
        // A recording's link: a joint without a key and without a record was destroyed before the snapshot (broken off):
        // Awake found it then, and the reference is a destroyed one now.
        let recording = !self.adopted_joints.is_empty();
        let key = |k: usize| self.break_joint_key(inst, (o, k)).or_else(|| (!recording).then(|| unimplemented!("mod-only: ReactorConduitBehaviour: a joint without a break limit key ({} #{})", prefab.objects[o].name, inst.objects[o].components[k])));
        let tube = joint(true).map(|(k, j)| (k, j, key(k)));
        let wall_joint = joint(false);
        let wall = wall_joint.as_ref().and_then(|&(k, _)| key(k));
        self.reactor_conduits[c].reactor_joint = wall;
        let mut tube_broken = false;
        if let Some((k, j, key)) = tube {
            self.reactor_conduits[c].tube = key.map(|key| (inst.objects[o].components[k], key));
            tube_broken = key.is_none();
            self.reactor_conduits[c].initialBreakForce = j.break_force;
            self.reactor_conduits[c].randomResilience = self.random.range_int(0, 100) as f32;
            let p = self.reactor_conduits[c].physical;
            self.physicals[p].ForceNoChargeParticles = true;
            let body = j.connected_body;
            // connectedConduit = tubeJoint.connectedBody.GetComponent<PhysicalBehaviour>(): the rest of Awake is skipped.
            if body.is_null() {
                super::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
            }
            if body.file != 0 {
                unimplemented!("mod-only: ReactorConduitBehaviour: a tube joint's connectedBody outside its prefab");
            }
            let at = prefab.index.get(&ObjRef { file: prefab.objects[o].game_object.file, path_id: body.path_id }).expect("ReactorConduitBehaviour: dangling connectedBody").0;
            self.reactor_conduits[c].connected = linked.physicals.get(&at).copied();
            self.reactor_conduits[c].connected_object = Some(at);
        }
        if let Some(t) = self.reactor_conduits[c].LiquidPushTarget {
            self.reactor_conduits[t].source = Some(c);
        }
        // A recording's broken-off joints: their OnJointBreak2D ran (after this Awake). The tube's: Broken, and a push target
        // it was connected to lets go; the wall's: HealthyWallJoint = false.
        if tube_broken {
            if let Some(t) = self.reactor_conduits[c].LiquidPushTarget
                && self.reactor_conduits[c].connected_object.and_then(|x| linked.reactor_conduits.get(&x)) == Some(&t)
            {
                self.reactor_conduits[t].source = None;
                self.reactor_conduits[c].LiquidPushTarget = None;
            }
            self.reactor_conduits[c].Broken = true;
        }
        if wall_joint.is_some() && wall.is_none() {
            self.reactor_conduits[c].HealthyWallJoint = false;
        }
    }

    /// CopperWireBehaviour.Awake (WireBehaviour.Awake's GetComponent<PhysicalBehaviour> is the link's): `typedJoint`, the
    /// prefab's DistanceJoint2D, is the component the wire holds from now on.
    fn copper_wire_awake_at(&mut self, w: usize, o: usize, v: &Value, prefab: &Prefab, inst: &Instance, linked: &Linked) {
        let p = field(v, "typedJoint").and_then(|x| x.pptr().ok_or_else(|| "typedJoint".to_string())).unwrap();
        assert!(!p.is_null(), "CopperWireBehaviour.typedJoint is null: Start adds the joint (DistanceJointWireBehaviour.Start)");
        let file = prefab.objects[o].game_object.file;
        let &(jo, jc) = prefab.index.get(&ObjRef { file, path_id: p.path_id }).expect("CopperWireBehaviour.typedJoint: dangling");
        let Component::Joint2D(j) = &prefab.objects[jo].components[jc].1 else { unimplemented!("mod-only: CopperWireBehaviour.typedJoint is no Joint2D") };
        let state = inst.anchored.get(&(jo, jc)).cloned().unwrap_or_else(|| anchored_initial_state(j));
        let distance = state.distance.expect("CopperWireBehaviour.typedJoint is no DistanceJoint2D");
        let physical = |o: usize| match linked.physicals.get(&o) {
            Some(&p) => BodyOwner::Physical(p),
            None => BodyOwner::Node(linked.node_rigidbodies[&o]),
        };
        let a = AutofixJoint { id: inst.objects[jo].components[jc], object: jo, component: jc, state: state.base, slider: None, joint: inst.joints.get(&(jo, jc)).copied(), slot: state.slot, attached: physical(jo), connected: joint_connected_object(prefab, jo, jc, j).map(physical), enabled: j.enabled };
        let f = self.adopt_prefab_joint(&a);
        self.fixed_joints[f].distance = Some(distance);
        self.blood_wires[w].joint = f;
    }

    /// IndustrialLiftBehaviour.Awake: the sliders A and B (their `Game::fixed_joints` entries), then dist.
    /// JointMotorSoundBehaviour's SliderJoint2D, adopted like the lift's (the same component is one entry).
    fn joint_motor_sound_awake_at(&mut self, i: usize, prefab: &Prefab, inst: &Instance, linked: &Linked) {
        let Some((o, c)) = self.joint_motor_sounds[i].slider_at else { return };
        let physical = |o: usize| match linked.physicals.get(&o) {
            Some(&p) => BodyOwner::Physical(p),
            None => BodyOwner::Node(linked.node_rigidbodies[&o]),
        };
        let Component::Joint2D(j) = &prefab.objects[o].components[c].1 else { unreachable!() };
        let state = inst.anchored.get(&(o, c)).cloned().unwrap_or_else(|| anchored_initial_state(j));
        // (A recording's loader adopted the recorded SliderJoint2Ds already: adopt_prefab_joint finds the entry.)
        if (state.slider.is_none() || state.slot.is_none()) && !self.adopted_joints.contains_key(&inst.objects[o].components[c]) {
            // A recording's link: a slider without a record was destroyed before the snapshot (a null reference now).
            if !self.adopted_joints.is_empty() {
                return;
            }
            unimplemented!("mod-only: JointMotorSoundBehaviour: a SliderJoint2D that was never created");
        }
        let a = AutofixJoint { id: inst.objects[o].components[c], object: o, component: c, state: state.base, slider: state.slider, joint: inst.joints.get(&(o, c)).copied(), slot: state.slot, attached: physical(o), connected: joint_connected_object(prefab, o, c, j).map(physical), enabled: j.enabled };
        self.joint_motor_sounds[i].slider = Some(self.adopt_prefab_joint(&a));
    }

    pub(crate) fn industrial_lift_awake_at(&mut self, i: usize, prefab: &Prefab, inst: &Instance, linked: &Linked) {
        let physical = |o: usize| match linked.physicals.get(&o) {
            Some(&p) => BodyOwner::Physical(p),
            None => BodyOwner::Node(linked.node_rigidbodies[&o]),
        };
        let mut joints = [None, None];
        for (k, at) in [self.industrial_lifts[i].a, self.industrial_lifts[i].b].into_iter().enumerate() {
            let Some((o, c)) = at else { continue };
            let Component::Joint2D(j) = &prefab.objects[o].components[c].1 else { unimplemented!("mod-only: IndustrialLiftBehaviour: a joint reference that is no Joint2D") };
            let state = inst.anchored.get(&(o, c)).cloned().unwrap_or_else(|| anchored_initial_state(j));
            if (state.slider.is_none() || state.slot.is_none()) && !self.adopted_joints.contains_key(&inst.objects[o].components[c]) {
                // A recording's link (it adopted the recorded sliders): one without a record was destroyed before the
                // snapshot (shot off), so the reference is null now (its dist is the recorded one).
                if !self.adopted_joints.is_empty() {
                    continue;
                }
                unimplemented!("mod-only: IndustrialLiftBehaviour: a SliderJoint2D that was never created ({} #{})", prefab.objects[o].name, inst.objects[o].components[c]);
            }
            let a = AutofixJoint { id: inst.objects[o].components[c], object: o, component: c, state: state.base, slider: state.slider, joint: inst.joints.get(&(o, c)).copied(), slot: state.slot, attached: physical(o), connected: joint_connected_object(prefab, o, c, j).map(physical), enabled: j.enabled };
            joints[k] = Some(self.adopt_prefab_joint(&a));
        }
        (self.industrial_lifts[i].a_joint, self.industrial_lifts[i].b_joint) = (joints[0], joints[1]);
        self.industrial_lift_awake(i);
    }

    /// The m_JointBreakLimits key of a created prefab joint (`Game::hinges` took a linked hinge's slot over).
    fn break_joint_key(&self, inst: &Instance, k: (usize, usize)) -> Option<BreakJoint> {
        // A recording's joint the loader adopted (a map's MapFixedJoint2D record): its `fixed_joints` entry.
        if let Some(&f) = self.adopted_joints.get(&inst.objects[k.0].components[k.1]) {
            return Some(BreakJoint::Fixed(f));
        }
        let slot = inst.hinges.get(&k).and_then(|s| s.slot).or_else(|| inst.anchored.get(&k).and_then(|s| s.slot))?;
        Some(match self.anchored_joints[slot].hinge {
            Some(h) => BreakJoint::Hinge(h),
            None => BreakJoint::Anchored(slot),
        })
    }

    /// The m_JointBreakLimits key of the joint component with instance id `id` on a clone (None: no such joint).
    pub(crate) fn break_joint_of_component(&self, id: i32) -> Option<BreakJoint> {
        self.unsynced.iter().chain(&self.retained).find_map(|inst| {
            let k = inst.objects.iter().enumerate().find_map(|(o, x)| x.components.iter().position(|&c| c == id).map(|k| (o, k)))?;
            Some(self.break_joint_key(inst, k))
        })?
    }

    /// `host.GetComponentsInChildren<HingeJoint2D>()`, `<SliderJoint2D>()` and `<FixedJoint2D>()`: the host and its active
    /// descendants, depth first, each object's components in order (disabled components included).
    fn autofix_components(&self, prefab: &Prefab, inst: &Instance, linked: &Linked, host: usize) -> AutofixJoints {
        let mut out = AutofixJoints::default();
        let physical = |o: usize| match linked.physicals.get(&o) {
            Some(&p) => BodyOwner::Physical(p),
            None => BodyOwner::Node(linked.node_rigidbodies[&o]),
        };
        let mut stack = vec![host];
        while let Some(o) = stack.pop() {
            if !inst.objects[o].active {
                continue;
            }
            stack.extend(prefab.objects[o].children.iter().rev());
            for (c, (_, comp)) in prefab.objects[o].components.iter().enumerate() {
                let Component::Joint2D(j) = comp else { continue };
                let list = match j.kind {
                    JointKind::Hinge { .. } => {
                        out.hinges.push(linked.hinges[&(o, c)]);
                        continue;
                    }
                    JointKind::Slider { .. } => &mut out.sliders,
                    JointKind::Fixed { .. } => &mut out.fixeds,
                    _ => continue,
                };
                let state = inst.anchored.get(&(o, c)).cloned().unwrap_or_else(|| anchored_initial_state(j));
                list.push(AutofixJoint {
                    id: inst.objects[o].components[c],
                    object: o,
                    component: c,
                    state: state.base,
                    slider: state.slider,
                    joint: inst.joints.get(&(o, c)).copied(),
                    slot: state.slot,
                    attached: physical(o),
                    connected: joint_connected_object(prefab, o, c, j).map(physical),
                    enabled: j.enabled,
                });
            }
        }
        out
    }

    /// Queues the Start of each behaviour in `order` (`run_pending_starts`, or PersonBehaviour's before its first
    /// LateUpdate). Each PhysicalBehaviour's `hash_code` must be set by then: Random.InitState(GetHashCode()) takes
    /// Mono's object hash. SkinMaterialHandler, DisintegrationCounterBehaviour and IncreaseStatOnStart's Starts only
    /// touch rendering, event subscriptions and the stat manager.
    pub fn queue_starts(&mut self, mut db: Option<&mut AssetDb>, prefab: &Prefab, inst: &Instance, linked: &Linked, order: &[(usize, usize)]) -> Result<(), String> {
        use super::misc::PendingStart;
        for &(o, c) in order {
            let Component::Script { class, fields: v, enabled, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
            if !inst.objects[o].active || !enabled {
                continue;
            }
            let limb = || linked.limbs[&o];
            let flag = |name: &str| field(v, name).and_then(|x| x.bool().ok_or_else(|| name.to_string())).unwrap();
            let start = match class.as_str() {
                "PersonBehaviour" => PendingStart::Person(linked.person.unwrap()),
                "HingeJointLimitAutofixBehaviour" => match linked.person {
                    Some(p) => PendingStart::HingeJointLimitAutofix(p),
                    None => {
                        let joints = self.autofix_components(prefab, inst, linked, o);
                        // Without a limited hinge or slider, or a FixedJoint2D, below it neither a flip nor Start touches anything.
                        if !joints.hinges.iter().any(|&h| self.hinges[h].useLimits) && !joints.sliders.iter().any(|j| j.slider.as_ref().unwrap().use_limits) && joints.fixeds.is_empty() {
                            continue;
                        }
                        let host = linked.physicals.get(&o).copied().ok_or_else(|| {
                            *linked.nodes.get(&o).unwrap_or_else(|| unimplemented!("mod-only: HingeJointLimitAutofixBehaviour on an object that is neither physical nor a Transform node"))
                        });
                        let mut joints = joints;
                        joints.id = inst.objects[o].components[c];
                        self.autofix_joints.push(joints);
                        PendingStart::HingeJointLimitAutofixItem(host, self.autofix_joints.len() - 1)
                    }
                },
                "CirculationBehaviour" => PendingStart::Circulation(limb()),
                "PhysicalBehaviour" => PendingStart::Physical(linked.physicals[&o]),
                "LimbBehaviour" => {
                    self.limbs[limb()].started = false;
                    PendingStart::Limb(limb())
                }
                "ConnectedNodeBehaviour" => PendingStart::Node(limb(), v.get("SerialisableConnectedNodes").and_then(Value::array).map(|a| a.iter().map(|x| x.bool().unwrap()).collect())),
                "GripBehaviour" => PendingStart::Grip(self.limbs[limb()].GripBehaviour.unwrap()),
                "GoreStringBehaviour" => {
                    self.script_slots.insert(inst.objects[o].components[c], super::save_slots::Slot::Gores(limb()));
                    PendingStart::GoreString(limb(), flag("CreateJointOnStart"))
                }
                "ShatteredObjectGenerator" if flag("GenerateOnStart") => unimplemented!("mod-only: ShatteredObjectGenerator.Start with GenerateOnStart"),
                "MapLightBehaviour" => PendingStart::MapLight(linked.map_lights[&o]),
                "WaterBehaviour" => PendingStart::Water(linked.waters[&o]),
                "DamagableMachineryBehaviour" => PendingStart::Machinery(linked.machinery[&o]),
                "DestroyableBehaviour" | "VaseBehaviour" => PendingStart::Destroyable(linked.destroyables[&o]),
                "UniversalBellBehaviour" => PendingStart::UniversalBell(linked.universal_bells[&o]),
                "LocalFirePropagationMap" => PendingStart::FireMap(linked.fire_maps[&o]),
                "NoChildrenCollide" => PendingStart::NoChildrenCollide(inst.objects[o].components[c]),
                "PlayAtRandom" => PendingStart::PlayAtRandom(inst.objects[o].components[c], float(v, "Chance")?),
                "AudioSourceTimeScaleBehaviour" => PendingStart::AudioTimeScale(inst.objects[o].components[0], flag("IsAmbience")),
                "DetachFromParentOnStart" => PendingStart::DetachFromParent(inst.objects[o].components[c], flag("DestroyIfParentDestroyed")),
                "ResetTransformBehaviour" if !flag("ResetOnStart") => continue,
                "ResetTransformBehaviour" => {
                    let physics = |x: usize| prefab.objects[x].components.iter().any(|(_, k)| matches!(k, Component::Rigidbody2D(_) | Component::Collider2D(_) | Component::Joint2D(_)));
                    if physics(o) || inst.descendants(o).into_iter().any(physics) {
                        unimplemented!("mod-only: ResetTransformBehaviour on an object with physics (the bodies would follow the transform)");
                    }
                    PendingStart::ResetTransform(inst.objects[o].components[c], field(v, "ToReset")?.i64().ok_or("ResetTransformBehaviour.ToReset")?)
                }
                "InstantiateObjectBehaviour" if !flag("InstantiateOnStart") => continue,
                "InstantiateObjectBehaviour" => PendingStart::InstantiateObject(inst.objects[o].components[c]),
                "RandomForceAtStart" => {
                    let k = prefab.objects[o].components.iter().position(|(_, x)| matches!(x, Component::Rigidbody2D(_))).ok_or("RandomForceAtStart without a Rigidbody2D")?;
                    PendingStart::RandomForce(inst.objects[o].components[k], float(v, "PositionalVelocityStrength")?, float(v, "RotationalVelocityStrength")?)
                }
                "DoAtTimeOnce" if !flag("StartSequenceOnStart") => continue,
                "DoAtTimeOnce" => {
                    let id = inst.objects[o].components[c];
                    PendingStart::DoAtTimeOnce(self.do_at_time_onces.iter().position(|d| d.id == id).unwrap())
                }
                // ExplosionSoundBehviour.Awake: GetComponent<AudioSource>() when audioSource is null, Global.AddAudioSource (audio).
                // ExplosionSoundBehviour.OnDestroy: Global.RemoveAudioSource, audio
                // Disabled at run time before its Start (set_enabled(false)): set_enabled(true) starts it.
                "ExplosionSoundBehviour" if self.disabled_ids.contains(&inst.objects[o].components[c]) => continue,
                "ExplosionSoundBehviour" if !flag("StartWhenAwake") => {
                    self.explosion_sounds_started.insert(inst.objects[o].components[c]);
                    continue;
                }
                "ExplosionSoundBehviour" => {
                    let set = field(v, "audioSource")?.pptr().is_some_and(|p| !p.is_null());
                    let has = set || prefab.objects[o].components.iter().any(|(_, x)| matches!(x, Component::AudioSource { .. }));
                    let near = array(v, "Near")?.len();
                    let distant = field(v, "DistantSound")?.i64().ok_or("DistantSound")?;
                    self.explosion_sounds_started.insert(inst.objects[o].components[c]);
                    PendingStart::ExplosionSoundPlay(inst.objects[o].components[c], has.then_some(near), distant, float(v, "Delay")?)
                }
                "PhysicalRopeCreator" if !flag("CreateOnStart") => continue,
                "PhysicalRopeCreator" => PendingStart::Rope(inst.objects[o].components[c]),
                "ExistByChance" => PendingStart::ExistByChance(inst.objects[o].components[c], float(v, "Chance")?),
                "UnderwaterExplosionBehaviour" => {
                    // SplashAudio: the ExplosionSoundBehviour that waits for Play; Splash is its object.
                    let splash = (0..prefab.objects.len())
                        .find(|&e| script(prefab, e, "ExplosionSoundBehviour").is_some_and(|(_, _, s)| field(s, "StartWhenAwake").is_ok_and(|x| x.bool() == Some(false))))
                        .ok_or("UnderwaterExplosionBehaviour without SplashAudio")?;
                    let (sc, _, sound) = script(prefab, splash, "ExplosionSoundBehviour").unwrap();
                    let distant = field(sound, "DistantSound")?.i64().ok_or("DistantSound")?;
                    // The behaviour with its serialised fields (CreateUnderwaterExplosionEffect sets them after Instantiate).
                    // (A later SetActive(true) queues Start again over the same behaviour.)
                    let (root, id) = (inst.objects[0].components[0], inst.objects[o].components[c]);
                    if !self.underwater_explosions.iter().any(|u| u.id == id) { self.underwater_explosions.push(super::explosion::UnderwaterExplosion {
                        root,
                        id,
                        position: inst.position(0),
                        GivenTop: float(v, "GivenTop")?,
                        MaxDistanceToSurface: float(v, "MaxDistanceToSurface")?,
                        DoSplash: flag("DoSplash"),
                        scale: 1.0,
                        ..Default::default()
                    }); }
                    PendingStart::UnderwaterExplosion(inst.objects[0].components[0], prefab.objects[splash].game_object, array(sound, "Near")?.len(), distant, inst.objects[splash].components[sc])
                }
                "OnlyExplosionBehaviour" if !flag("ExplodeOnStart") => continue,
                "OnlyExplosionBehaviour" => PendingStart::OnlyExplosion(inst.objects[o].components[c], explosion_parameters(field(v, "ExplosionParameters")?)?),
                "ExplosiveBehaviour" => PendingStart::Explosive(linked.explosives[&o]),
                "NoCollide" => PendingStart::NoCollide(inst.objects[o].components[c]),
                "CarBehaviour" => PendingStart::Car(linked.cars[&o]),
                "TyreDeflationBehaviour" => PendingStart::Tyre(linked.tyres[&o]),
                "FirearmBehaviour" => PendingStart::Firearm(linked.firearms[&o]),
                "MachineGunBehaviour" => PendingStart::MachineGun(linked.machine_guns[&o]),
                "FirearmAttachmentPointBehaviour" => {
                    if field(v, "FirearmAttachment")?.pptr().is_some_and(|p| !p.is_null()) {
                        unimplemented!("mod-only: FirearmAttachmentPointBehaviour: a prefab with a serialized FirearmAttachment (every item prefab holds null; a pasted one is linked by ObjectState)");
                    }
                    PendingStart::FirearmAttachmentPoint(inst.objects[o].components[c])
                }
                "GasTankBehaviour" | "FlaskBehaviour" => PendingStart::Flask(linked.flasks[&o]),
                c if super::syringe::is_syringe(c) => PendingStart::Syringe(linked.syringes[&o]),
                "SyringeExplosionBehaviour" => PendingStart::SyringeExplosion(linked.syringe_explosions[&o]),
                "LiquidContainerController" => PendingStart::LiquidContainer(linked.liquid_containers[&o]),
                "DetectorBehaviour" => PendingStart::Detector(linked.detectors[&o]),
                "ButtonBehaviour" => PendingStart::Button(linked.buttons[&o]),
                "TelevisionBehaviour" => PendingStart::Television(linked.televisions[&o]),
                "ParticleSystemSerialisationHelper" => PendingStart::SerialisationHelper(self.serialisation_helpers.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap()),
                "MuteBehaviour" => {
                    // Start: GetComponent<PhysicalBehaviour>().ContextMenuOptions throws before anything else.
                    match linked.physicals.get(&o) {
                        None => PendingStart::Throw("NullReferenceException: Object reference not set to an instance of an object".into()),
                        Some(&p) => {
                            self.mutes.push(flag("Muted"));
                            self.mute_ids.push(inst.objects[o].components[c]);
                            self.script_slots.insert(inst.objects[o].components[c], super::save_slots::Slot::Mutes(self.mutes.len() - 1));
                            PendingStart::Mute(self.mutes.len() - 1, p)
                        }
                    }
                }
                "LaserBehaviour" => PendingStart::Laser(linked.lasers[&o]),
                "HeatingElementBehaviour" => PendingStart::Menu(super::misc::Menu::HeatingElement(linked.heating_elements[&o])),
                "LiquidPumpBehaviour" => PendingStart::LiquidPump(linked.liquid_pumps[&o]),
                "HovercarBehaviour" => PendingStart::Menu(super::misc::Menu::Hovercar(linked.hovercars[&o])),
                "LEDBulbBehaviour" => PendingStart::Menu(super::misc::Menu::Led(linked.leds[&o])),
                // LiquidentifierBehaviour.Start: the Flush button (menu.rs).
                "LiquidentifierBehaviour" => PendingStart::Menu(super::misc::Menu::Liquidentifier(linked.liquidentifiers[&o])),
                // LiquidDuplicatorBehaviour.Start: the Flush button (menu.rs).
                "LiquidDuplicatorBehaviour" => PendingStart::Menu(super::misc::Menu::LiquidDuplicator(linked.liquid_duplicators[&o])),
                "LaserReceiverBehaviour" => PendingStart::Menu(super::misc::Menu::LaserReceiver(linked.laser_receivers[&o])),
                "LiquidLevelGateBehaviour" => PendingStart::Menu(super::misc::Menu::LiquidLevelGate(linked.liquid_level_gates[&o])),
                "KeyTriggerBehaviour" => PendingStart::Menu(super::misc::Menu::KeyTrigger(linked.key_triggers[&o])),
                "CounterBehaviour" => PendingStart::Menu(super::misc::Menu::Counter(linked.counters[&o])),
                "PointerBehaviour" => PendingStart::Menu(super::misc::Menu::Pointer(linked.pointers[&o])),
                "LaserTurretBehaviour" => PendingStart::LaserTurret(linked.laser_turrets[&o]),
                "SentryTurretBehaviour" => PendingStart::SentryTurret(linked.sentry_turrets[&o]),
                "SingleFloodlightBehaviour" => PendingStart::Floodlight(linked.floodlights[&o]),
                "GeneratorBehaviour" => PendingStart::Generator(linked.generators[&o]),
                "SpikeGrenadeBehaviour" => PendingStart::SpikeGrenade(linked.spike_grenades[&o]),
                "BulbBehaviour" => PendingStart::Bulb(linked.bulbs[&o]),
                "CathodeLightBehaviour" => PendingStart::CathodeLight(linked.cathode_lights[&o]),
                "TranslationAnimationBehaviour" | "RotationAnimationBehaviour" | "ColorAnimationBehaviour" | "SpriteGradientAnimation" => PendingStart::Visual(linked.visuals[&(o, c)]),
                "GlowtubeBehaviour" => PendingStart::Glowtube(linked.glowtubes[&o]),
                "ValveBehaviour" | "LiquidPressureValveBehaviour" => PendingStart::Valve(linked.valves[&o]),
                "CentrifugeBehaviour" => PendingStart::Centrifuge(linked.centrifuges[&o]),
                "ActivatorElectrodeBehaviour" => PendingStart::ActivatorElectrode(linked.activator_electrodes[&o]),
                "ChainsawBehaviour" => PendingStart::Chainsaw(linked.chainsaws[&o]),
                "FreezeBehaviour" => PendingStart::Freeze(linked.freezes[&o]),
                "FreezeSerialisationHelper" => PendingStart::FreezeSerialisation(linked.freeze_serialisations[&o]),
                "CeilingTurretBehaviour" => PendingStart::CeilingTurret(linked.ceiling_turrets[&o]),
                "WormStaffBehaviour" => PendingStart::WormStaff(linked.worm_staffs[&o]),
                "NewAtomBombExplosionBehaviour" => PendingStart::AtomExplosion(linked.atom_explosions[&o]),
                "DecimatorBehaviour" => PendingStart::Decimator(linked.decimators[&o]),
                "ObstructionEmitterBehaviour" => PendingStart::ObstructionEmitter(linked.obstruction_emitters[&o]),
                "HeartMonitorBehaviour" => PendingStart::HeartMonitor(linked.heart_monitors[&o]),
                "RayCannonBehaviour" => PendingStart::RayCannon(linked.ray_cannons[&o]),
                "BeamformerBehaviour" => PendingStart::Beamformer(linked.beamformers[&o]),
                "DroneBehaviour" => PendingStart::Drone(linked.drones[&o]),
                "CustomFixedConstrainedBehaviour" => PendingStart::CustomFixedConstrained(self.custom_fixeds.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap()),
                "PortalBehaviour" => PendingStart::Portal(linked.portals[&o]),
                "ImmobilityFieldBehaviour" => PendingStart::ImmobilityField(linked.immobility_fields[&o]),
                "JavelinBehaviour" => PendingStart::Javelin(linked.javelins[&o]),
                "JavelinMissileBehaviour" => PendingStart::JavelinMissile(linked.javelin_missiles[&o]),
                "TeslaCoilBehaviour" => PendingStart::TeslaCoil(linked.tesla_coils[&o]),
                "EnergySwordBehaviour" => PendingStart::EnergySword(linked.energy_swords[&o]),
                "ThrusterbedBehaviour" => PendingStart::Thrusterbed(linked.thrusterbeds[&o]),
                // Start reads Slider.limits: the component's state, taken from the spawn when the Start runs.
                "PistonBehaviour" => {
                    let mut joints = self.autofix_components(prefab, inst, linked, o);
                    joints.sliders.retain(|a| a.object == o);
                    if joints.sliders.len() != 1 {
                        unimplemented!("mod-only: PistonBehaviour without exactly one SliderJoint2D on its object");
                    }
                    joints.hinges.clear();
                    joints.fixeds.clear();
                    self.autofix_joints.push(joints);
                    PendingStart::Piston(linked.pistons[&o], self.autofix_joints.len() - 1)
                }
                "AdhesiveCouplerBehaviour" => PendingStart::Coupler(linked.couplers[&o]),
                "SeismicChargeExplosionBehaviour" => PendingStart::SeismicExplosion(linked.seismic_explosions[&o]),
                "TransmitterBehaviour" => PendingStart::Transmitter(linked.transmitters[&o]),
                "ThermometerBehaviour" => PendingStart::TextStart("ThermometerBehaviour", linked.thermometers[&o]),
                "TextMeshProControlReplacer" if linked.control_replacers.contains_key(&o) => PendingStart::TextStart("TextMeshProControlReplacer", linked.control_replacers[&o]),
                "DefibrillatorBehaviour" => PendingStart::TextStart("DefibrillatorBehaviour", linked.defibrillators[&o]),
                "LiquidCanisterBehaviour" => PendingStart::TextStart("LiquidCanisterBehaviour", linked.liquid_canisters[&o]),
                "IonBoltBehaviour" => PendingStart::IonBolt(linked.ion_bolts[&o]),
                "WinchBehaviour" => {
                    let mut joints = self.autofix_components(prefab, inst, linked, o);
                    joints.sliders.retain(|a| a.object == o);
                    if joints.sliders.len() != 1 {
                        unimplemented!("mod-only: WinchBehaviour without exactly one SliderJoint2D on its object");
                    }
                    joints.hinges.clear();
                    joints.fixeds.clear();
                    self.autofix_joints.push(joints);
                    self.winches[linked.winches[&o]].autofix = self.autofix_joints.len() - 1;
                    PendingStart::Winch(linked.winches[&o], self.autofix_joints.len() - 1)
                }
                "ConveyorBeltBehaviour" => PendingStart::Conveyor(linked.conveyors[&o]),
                "ReactorCoreBehaviour" => PendingStart::ReactorCore(linked.reactor_cores[&o]),
                "CopperWireBehaviour" | "NewUseWireBehaviour" => PendingStart::BloodWire(linked.copper_wires[&o]),
                "ReactorConduitSetBehaviour" => PendingStart::ReactorConduitSet(linked.reactor_conduit_sets[&o]),
                "RandomSoundBehaviour" => PendingStart::RandomSound(linked.random_sounds[&o]),
                "PlayRandomClipBehaviour" => PendingStart::PlayRandomClip(linked.play_random_clips[&o]),
                "MapLiftBehaviour" => {
                    let mut joints = self.autofix_components(prefab, inst, linked, o);
                    joints.sliders.retain(|a| a.object == o);
                    if joints.sliders.len() != 1 {
                        unimplemented!("mod-only: MapLiftBehaviour without exactly one SliderJoint2D on its object");
                    }
                    joints.hinges.clear();
                    joints.fixeds.clear();
                    self.autofix_joints.push(joints);
                    PendingStart::MapLift(linked.map_lifts[&o], self.autofix_joints.len() - 1)
                }
                "RandomCarTextureBehaviour" => PendingStart::CarTexture(self.random_car_textures.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap()),
                "DisintegrationCounterBehaviour" if linked.person.is_some() && o == 0 => continue,
                "DisintegrationCounterBehaviour" => PendingStart::DisintegrationCounter(inst.objects[o].components[c]),
                "ExistInDetailView" => PendingStart::ExistInDetailView(inst.objects[o].components[c]),
                "ExistInBeta" => PendingStart::ExistInBeta(inst.objects[o].components[c]),
                "ExistInThermalVision" => PendingStart::ExistInThermalVision(inst.objects[o].components[c]),
                // InvisibleOptionInContextMenu.Awake: Invisible = !SpriteRenderer.enabled, phys and the LimbStatusToggled subscription (toggle_limb_status)
                // InvisibleOptionInContextMenu.OnDestroy: the unsubscription (a destroyed one is skipped there)
                "InvisibleOptionInContextMenu" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let sprite_enabled = prefab.objects[o].components.iter().find_map(|(_, x)| match x {
                        Component::SpriteRenderer(r) => Some(r.enabled),
                        _ => None,
                    }).unwrap_or_else(|| unimplemented!("mod-only: InvisibleOptionInContextMenu without a SpriteRenderer (NullReferenceException in Awake)"));
                    let physical = *linked.physicals.get(&o).unwrap_or_else(|| unimplemented!("mod-only: InvisibleOptionInContextMenu without a PhysicalBehaviour (NullReferenceException in Start)"));
                    let detail = v.get("VisibleInDetailView").and_then(Value::bool).expect("InvisibleOptionInContextMenu.VisibleInDetailView");
                    self.invisibles.push(super::misc::Invisible { id: inst.objects[o].components[c], physical: Some(physical), Invisible: !sprite_enabled, VisibleInDetailView: detail, sprite_enabled });
                    self.script_slots.insert(inst.objects[o].components[c], super::save_slots::Slot::Invisibles(self.invisibles.len() - 1));
                    PendingStart::Menu(super::misc::Menu::Invisible(self.invisibles.len() - 1))
                }
                "IgnoreParentSize" => PendingStart::IgnoreParentSize(inst.objects[o].components[c]),
                "RandomSpriteBehaviour" => PendingStart::RandomSprite(linked.random_sprites[&o]),
                "AirfoilBehaviour" => PendingStart::Airfoil(linked.airfoils[&o]),
                "StaticNeedleBehaviour" => PendingStart::StaticNeedle(self.static_needles.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap()),
                "ShockwaveBehaviour" => PendingStart::Shockwave(self.shockwaves.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap()),
                "MetronomeBehaviour" => PendingStart::Metronome(linked.metronomes[&o]),
                name if super::v45_new::LINKED.contains(&name) && crate::assets::il::is_1_28_45() => match self.v45_pending_start(inst.objects[o].components[c]) {
                    Some(start) => start,
                    None => continue,
                },
                "BlasterBehaviour" => PendingStart::Blaster(linked.blasters[&o]),
                "SmoothLineControllerBehaviour" => PendingStart::SmoothLine(self.smooth_lines.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap()),
                "DeterminedAnomaly" => PendingStart::DeterminedAnomaly(linked.determined_anomalies[&o]),
                "StartAudioSourceAtRandomTimeBehaviour" => PendingStart::StartAudioSourceAtRandomTime(inst.objects[o].components[c]),
                "ArrayCloner" => {
                    let delta = field(v, "Delta")?;
                    PendingStart::ArrayCloner(inst.objects[o].components[c], field(v, "Amount")?.i64().ok_or("ArrayCloner.Amount")? as i32, [float(delta, "x")?, float(delta, "y")?])
                }
                "TimedGateBehaviour" => PendingStart::TimedGate(linked.timed_gates[&o]),
                "JukeboxBehaviour" => {
                    PendingStart::Menu(super::misc::Menu::Jukebox(linked.jukeboxes[&o]))
                }
                "HoverThrusterBehaviour" => PendingStart::Menu(super::misc::Menu::HoverThruster(linked.hover_thrusters[&o])),
                "HeartMonitorBehaviour" => PendingStart::Menu(super::misc::Menu::HeartMonitor(linked.heart_monitors[&o])),
                "DisplayBehaviour" => PendingStart::Menu(super::misc::Menu::Display(linked.displays[&o])),
                // Awake's GetComponent<PhysicalBehaviour>()
                "EditableTintBehaviour" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let mut tint = [0.0f32; 4];
                    super::Field::set(&mut tint, v.get("Tint").ok_or("EditableTintBehaviour.Tint")?)?;
                    // Start: phys.ContextMenuOptions throws (nothing else reads phys but the button it never adds).
                    match linked.physicals.get(&o) {
                        None => PendingStart::Throw("NullReferenceException: Object reference not set to an instance of an object".into()),
                        Some(&p) => {
                            self.editable_tints.push(tint);
                            self.editable_tint_phys.push(p);
                            self.script_slots.insert(inst.objects[o].components[c], super::save_slots::Slot::EditableTints(self.editable_tints.len() - 1));
                            PendingStart::Menu(super::misc::Menu::EditableTint(self.editable_tints.len() - 1, p))
                        }
                    }
                }
                "ParticleMachineBehaviour" => {
                    let Component::Script { fields: v, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
                    let flag = |n: &str| v.get(n).and_then(Value::bool).unwrap_or_else(|| panic!("ParticleMachineBehaviour.{n}"));
                    let emit = [flag("EmitSmoke"), flag("EmitFire"), flag("EmitElectricity")];
                    // Start: addToggleButton's phys.ContextMenuOptions throws at the first button.
                    match linked.physicals.get(&o) {
                        None => PendingStart::Throw("NullReferenceException: Object reference not set to an instance of an object".into()),
                        Some(&p) => {
                            self.particle_machine_emit.push(emit);
                            self.script_slots.insert(inst.objects[o].components[c], super::save_slots::Slot::ParticleMachines(self.particle_machine_emit.len() - 1));
                            PendingStart::Menu(super::misc::Menu::ParticleMachine(self.particle_machine_emit.len() - 1, p))
                        }
                    }
                }
                "GateBehaviour" => PendingStart::Menu(super::misc::Menu::Gate(linked.gates[&o])),
                "LiquidContainerLeakingBehaviour" => PendingStart::Menu(super::misc::Menu::LiquidLeak(self.liquid_leaks.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap())),
                "LagboxBehaviour" => PendingStart::Lagbox(linked.lagboxes[&o]),
                "FireworksBehaviour" => PendingStart::Fireworks(linked.fireworks[&o]),
                "CardiopulmonaryBypassMachineBehaviour" => PendingStart::CardiopulmonaryBypass(linked.cardiopulmonary_bypasses[&o]),
                "BloodTankBehaviour" => PendingStart::BloodTank(linked.blood_tanks[&o]),
                "ArchelixCasterBehaviour" => PendingStart::ArchelixCaster(linked.archelix_casters[&o]),
                "ArchelixCasterBoltBehaviour" => PendingStart::ArchelixBolt(linked.archelix_bolts[&o]),
                "MaintainScaleSignBehaviour" => PendingStart::MaintainScale(inst.objects[o].components[c]),
                "FancyBloodSplatController" => PendingStart::Splat(self.splats.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap()),
                "LiquidSpreadAoEBehaviour" => PendingStart::LiquidSpread(self.liquid_spreads.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap()),
                "FoliageBehaviour" => PendingStart::Foliage(self.foliages.iter().position(|x| x.id == inst.objects[o].components[c]).unwrap()),
                "ActivationToggleBehaviour" => PendingStart::ActivationToggle(linked.activation_toggles[&o]),
                "PowerGaugeBehaviour" | "PressureGaugeBehaviour" | "TemperatureGaugeBehaviour" => PendingStart::PowerGauge(linked.power_gauges[&o]),
                // A Signal Flare that starts Activated enables its TorchBehaviour (not rewritten); otherwise Start does nothing.
                "FlareBehaviour" => PendingStart::Flare(linked.flares[&o]),
                // CameraControlBehaviour takes the box.
                "BoundingBoxBehaviour" => PendingStart::BoundingBox(map_bounds(v)?),
                "MapConfig" => {
                    // MapConfig.Awake: Bounds = GetComponent<BoundingBoxBehaviour>() (read below).
                    self.map_config_present = true;
                    let db = db.as_deref_mut().unwrap_or_else(|| unimplemented!("mod-only: MapConfig.Start on a first GameObject.SetActive(true)"));
                    let file = prefab.objects[o].game_object.file;
                    let mut weather = Vec::new();
                    for prefab_field in ["SnowPrefab", "RainPrefab", "FogPrefab", "LightningCreatorPrefab"] {
                        let p = field(v, prefab_field)?.pptr().ok_or(prefab_field)?;
                        weather.push(match p.is_null() {
                            true => None,
                            false => Some(std::sync::Arc::new(db.prefab(db.resolve(file, p).ok_or_else(|| format!("{prefab_field} outside the data"))?)?)),
                        });
                    }
                    let lightning = weather.pop().unwrap();
                    // Awake's GetComponent<BoundingBoxBehaviour>().
                    let (_, _, b) = script(prefab, o, "BoundingBoxBehaviour").ok_or("MapConfig without a BoundingBoxBehaviour")?;
                    let ambience = field(v, "Ambience")?;
                    let settings = field(v, "Settings")?;
                    let ambience_audio = match field(v, "OverrideAmbientSounds")?.bool() {
                        Some(true) => Some((float(ambience, "Volume")?, array(ambience, "Clips")?.iter().map(|c| super::audio::clip_ref(db, &mut self.audio.clip_cache, c.pptr().and_then(|p| db.resolve(file, p)))).collect::<Result<Vec<_>, _>>()?)),
                        _ => None,
                    };
                    PendingStart::MapConfig(super::map::MapConfigStart {
                        ambience_audio,
                        bounds: map_bounds(b)?,
                        audio: linked.parent_audio,
                        ambience: field(v, "OverrideAmbientSounds")?.bool().ok_or("OverrideAmbientSounds")?.then(|| -> Result<_, String> { Ok((float(ambience, "Chance")?, array(ambience, "Clips")?.len())) }).transpose()?,
                        settings: super::map::EnvironmentalSettings {
                            Floodlights: field(settings, "Floodlights")?.bool().ok_or("Floodlights")?,
                            Rain: field(settings, "Rain")?.bool().ok_or("Rain")?,
                            Snow: field(settings, "Snow")?.bool().ok_or("Snow")?,
                            Fog: field(settings, "Fog")?.bool().ok_or("Fog")?,
                            Gravity: float(settings, "Gravity")?,
                            Lightning_chance: float(settings, "Lightning_chance")?,
                            Ambient_temperature: float(settings, "Ambient_temperature")?,
                        },
                        weather,
                        lightning,
                        original_ids: std::sync::Arc::new(db.resource_load_order()?),
                        transform: inst.objects[o].components[0],
                        rain_loop: super::audio::clip_ref(db, &mut self.audio.clip_cache, field(v, "RainLoop")?.pptr().and_then(|p| db.resolve(file, p)))?,
                    })
                }
                name if super::translated::CLASSES.contains(&name) => match linked.translated[&(o, c)] {
                    b if b.has_start() => PendingStart::Translated(b),
                    _ => continue,
                },
                "IncreaseStatOnStart" => match field(v, "StatName")? {
                    Value::Str(name) => PendingStart::IncreaseStat(name.clone(), flag("IsFloat")),
                    _ => return Err("IncreaseStatOnStart.StatName".into()),
                },
                _ => continue,
            };
            self.pending_starts.push(start);
        }
        Ok(())
    }

    /// The PhysicalProperties asset's arena index, loading it on first use (assets are shared by name).
    fn physical_properties(&mut self, db: &mut AssetDb, asset: ObjRef) -> Result<usize, String> {
        let (_, mb, _) = db.mono(asset)?.ok_or("PhysicalProperties is not a MonoBehaviour")?;
        if let Some(i) = self.properties.iter().position(|p| p.name == mb.name) {
            return Ok(i);
        }
        let p = read_physical_properties(db, asset)?;
        self.properties.push(p);
        Ok(self.properties.len() - 1)
    }
}

/// PhysicalProperties..ctor (SharpForceThresholdMultiplier, LodgeStrengthMultiplier, HeatTransferSpeedMultiplier,
/// ImpactIntensityMutliplier and HitVolumeMultiplier 1, BurningTemperatureThreshold 5000), then the asset's serialized
/// fields, which hold every one of them: a PhysicalProperties asset as the game reads it.
pub(super) fn read_physical_properties(db: &mut AssetDb, asset: ObjRef) -> Result<PhysicalProperties, String> {
    {
        let (_, mb, v) = db.mono(asset)?.ok_or("PhysicalProperties is not a MonoBehaviour")?;
        let b = |n: &str| -> Result<bool, String> { field(&v, n)?.bool().ok_or(n.to_string()) };
        let mut p = PhysicalProperties { name: mb.name.clone(), ..Default::default() };
        p.Softness = float(&v, "Softness")?;
        p.Flammability = float(&v, "Flammability")?;
        p.Conducting = b("Conducting")?;
        p.MagneticAttractionIntensity = float(&v, "MagneticAttractionIntensity")?;
        p.Burnrate = float(&v, "Burnrate")?;
        p.BurningTemperatureThreshold = float(&v, "BurningTemperatureThreshold")?;
        p.HeatTransferSpeedMultiplier = float(&v, "HeatTransferSpeedMultiplier")?;
        p.Buoyancy = float(&v, "Buoyancy")?;
        p.ImpactIntensityMutliplier = float(&v, "ImpactIntensityMutliplier")?;
        p.HitVolumeMultiplier = float(&v, "HitVolumeMultiplier")?;
        p.SparksOnSlide = b("SparksOnSlide")?;
        p.SoftImpact = array(&v, "SoftImpact")?.len();
        p.HardImpact = array(&v, "HardImpact")?.len();
        // Serialized arrays load as arrays, never null.
        p.SizzleSounds = Some(array(&v, "SizzleSounds")?.len());
        p.Sharp = b("Sharp")?;
        p.IgnoreStabResistance = b("IgnoreStabResistance")?;
        p.SharpForceThresholdMultiplier = float(&v, "SharpForceThresholdMultiplier")?;
        for a in array(&v, "SharpAxes")? {
            p.SharpAxes.push(SharpAxis {
                Axis: floats(field(a, "Axis")?)?,
                LowerLimit: float(a, "LowerLimit")?,
                UpperLimit: float(a, "UpperLimit")?,
                LooseUpperLimit: field(a, "LooseUpperLimit")?.bool().ok_or("LooseUpperLimit")?,
                LooseLowerLimit: field(a, "LooseLowerLimit")?.bool().ok_or("LooseLowerLimit")?,
            });
        }
        p.LodgeStrengthMultiplier = float(&v, "LodgeStrengthMultiplier")?;
        p.StabSound = Some(array(&v, "StabSound")?.len());
        p.Brittleness = float(&v, "Brittleness")?;
        p.BulletSpeedAbsorptionPower = float(&v, "BulletSpeedAbsorptionPower")?;
        p.ShotImpact = match db.resolve(asset.file, field(&v, "ShotImpact")?.pptr().ok_or("ShotImpact")?) {
            Some(r) => Some((Some(r), db.game_object(r)?.name == "BloodPuff poolable")),
            None => None,
        };
        match db.resolve(asset.file, field(&v, "PhysicMaterial")?.pptr().ok_or("PhysicMaterial")?) {
            Some(m) => {
                let m = PhysicsMaterial2D::read(db.data(m))?;
                (p.friction, p.bounciness) = (Some(m.friction), m.bounciness);
            }
            None => (p.friction, p.bounciness) = (None, 0.0),
        }
        let mut cache = std::collections::HashMap::new();
        let clips = |n: &str, cache: &mut _| audio_clips(db, cache, asset.file, &v, n);
        p.clips = super::physical::PropertyClips {
            SoftImpact: clips("SoftImpact", &mut cache)?,
            HardImpact: clips("HardImpact", &mut cache)?,
            SizzleSounds: clips("SizzleSounds", &mut cache)?,
            StabSound: clips("StabSound", &mut cache)?,
            SlidingLoop: super::audio::clip_ref(db, &mut cache, db.resolve(asset.file, field(&v, "SlidingLoop")?.pptr().ok_or("SlidingLoop")?))?,
        };
        Ok(p)
    }
}

impl Game {
    /// The SpriteRenderer on prefab object `t` as the game draws it (`GetComponent<SpriteRenderer>()`); None without one.
    fn sprite_at(&self, l: &Linker, t: usize) -> Option<super::visual::SpriteAt> {
        use super::visual::SpriteAt;
        let k = l.prefab.objects[t].components.iter().position(|(_, y)| matches!(y, Component::SpriteRenderer(_)))?;
        if let Some(&p) = l.arenas.physical.get(&t) {
            return Some(SpriteAt::Physical(p));
        }
        let rid = l.inst.objects[t].components[k];
        let r = self.renderers.iter().position(|r| r.id == rid).unwrap_or_else(|| unimplemented!("mod-only: {}: a SpriteRenderer without a sprite", l.prefab.objects[t].name));
        Some(SpriteAt::Renderer(r))
    }

    /// The renderers below object `o` (a subtree without physics) drawn through its local Transform: (renderer, chain index).
    fn subtree_renderers(&self, l: &Linker, o: usize) -> Vec<(usize, usize)> {
        let (prefab, inst) = (l.prefab, l.inst);
        let mut refs = Vec::new();
        for x in std::iter::once(o).chain(inst.descendants(o)) {
            for (k, (_, y)) in prefab.objects[x].components.iter().enumerate() {
                if !matches!(y, Component::SpriteRenderer(_)) {
                    continue;
                }
                let rid = inst.objects[x].components[k];
                let Some(r) = self.renderers.iter().position(|r| r.id == rid) else { continue };
                let (mut depth, mut t) = (0, x);
                while t != o {
                    t = inst.objects[t].parent.expect("a descendant's parent");
                    depth += 1;
                }
                refs.push((r, depth));
            }
        }
        refs
    }

    /// A serialized GameObject reference to a subtree that only draws (no script, collider, body or joint below it): its
    /// Transform id, for `Game::set_active_plain`. None for a null reference.
    fn plain_field(&self, l: &Linker, v: &Value, name: &str) -> Result<Option<i32>, String> {
        let Some(t) = l.object(field(v, name)?)? else { return Ok(None) };
        for d in std::iter::once(t).chain(l.inst.descendants(t)) {
            if l.prefab.objects[d].components.iter().any(|(_, c)| matches!(c, Component::Script { class, .. } if class != "Optout") || matches!(c, Component::Collider2D(_) | Component::Rigidbody2D(_) | Component::Joint2D(_) | Component::PointEffector2D(_))) {
                unimplemented!("mod-only: {name}: SetActive on an object with scripts or physics");
            }
        }
        Ok(Some(l.inst.objects[t].components[0]))
    }

    /// A serialized TextMeshPro (or TMP_Text) reference: its index in Game.world_texts.
    pub(super) fn text_field(&self, l: &Linker, v: &Value, name: &str) -> Result<Option<usize>, String> {
        let p = field(v, name)?.pptr().ok_or_else(|| format!("{name}: not a reference"))?;
        if p.is_null() {
            return Ok(None);
        }
        let Some(&(o, c)) = l.prefab.index.get(&ObjRef { file: l.file, path_id: p.path_id }).filter(|_| p.file == 0) else { return Err(format!("{name}: a reference outside the prefab")) };
        let id = l.inst.objects[o].components[c];
        Ok(Some(self.world_texts.iter().position(|t| t.id == id).ok_or_else(|| format!("{name}: not a TextMeshPro"))?))
    }

    /// A serialized SpriteRenderer reference.
    fn sprite_field(&self, l: &Linker, v: &Value, name: &str) -> Result<Option<super::visual::SpriteAt>, String> {
        Ok(match l.object(field(v, name)?)? {
            None => None,
            Some(t) => Some(self.sprite_at(l, t).ok_or_else(|| format!("{name}: no SpriteRenderer"))?),
        })
    }

    /// The visual scripts of the instance (visual.rs), by (object, component), with Awake's work done: TranslationAnimationBehaviour's
    /// startPos and ColorAnimationBehaviour's spriteRenderer; TrivialAnimationActivationBehaviour's phys.
    fn link_visuals(&mut self, l: &Linker, anchor_of: &dyn Fn(usize) -> (BareAnchor, Vec<crate::unity::transform::Trs>)) -> Result<HashMap<(usize, usize), usize>, String> {
        use super::visual::{Anim, Gradient, Kind, Visual};
        let (prefab, inst) = (l.prefab, l.inst);
        let mut out = HashMap::new();
        let mut activations = Vec::new();
        for o in 0..prefab.objects.len() {
            for (c, (_, comp)) in prefab.objects[o].components.iter().enumerate() {
                let Component::Script { class, enabled, fields: v, .. } = comp else { continue };
                let id = inst.objects[o].components[c];
                let curve_at = |name: &str| -> Result<(Curve, CurveCache), String> { Ok((curve(field(v, name)?)?, CurveCache::default())) };
                let flag = |name: &str| field(v, name).and_then(|x| x.bool().ok_or_else(|| name.to_string()));
                let kind = match class.as_str() {
                    "TranslationAnimationBehaviour" | "RotationAnimationBehaviour" | "ColorAnimationBehaviour" => {
                        let local = inst.objects[o].trs;
                        let anim = match class.as_str() {
                            "TranslationAnimationBehaviour" => {
                                let (curve, cache) = curve_at("DistanceOverTime")?;
                                let vertical = match field(v, "Axis")?.i64() {
                                    Some(0) => false,
                                    Some(1) => true,
                                    x => return Err(format!("TranslationAnimationBehaviour.Axis {x:?}")),
                                };
                                Anim::Translation { curve, cache, vertical, Multiplier: float(v, "Multiplier")?, startPos: translation_animation_awake([local.t[0], local.t[1], local.t[2]]) }
                            }
                            "RotationAnimationBehaviour" => {
                                let (curve, cache) = curve_at("DegreeOverTime")?;
                                let [ox, oy, oz] = floats::<3>(field(v, "Offset")?)?;
                                let [mx, my, mz] = floats::<3>(field(v, "Multiplier")?)?;
                                if ox != 0.0 || oy != 0.0 || mx != 0.0 || my != 0.0 {
                                    unimplemented!("mod-only: RotationAnimationBehaviour about x or y (localEulerAngles beyond z)");
                                }
                                Anim::Rotation { curve, cache, Offset: oz, Multiplier: mz }
                            }
                            _ => Anim::Color { gradient: Gradient::read(field(v, "ColourOverTime")?)?, sprite: self.sprite_at(l, o) },
                        };
                        let mut refs = Vec::new();
                        if !matches!(anim, Anim::Color { .. }) {
                            for x in std::iter::once(o).chain(inst.descendants(o)) {
                                if l.arenas.physical.contains_key(&x) || l.arenas.node.contains_key(&x) || prefab.objects[x].components.iter().any(|(_, k)| matches!(k, Component::Collider2D(_) | Component::Rigidbody2D(_) | Component::Joint2D(_))) {
                                    unimplemented!("mod-only: {class} over a subtree with physics");
                                }
                            }
                            refs = self.subtree_renderers(l, o);
                        }
                        Kind::Trivial { Duration: float(v, "Duration")?, Loop: flag("Loop")?, PlayOnStart: flag("PlayOnStart")?, IsPlaying: false, anim, local, refs }
                    }
                    "TrivialAnimationActivationBehaviour" => {
                        let target = field(v, "AnimationBehaviour")?.pptr().ok_or("AnimationBehaviour")?;
                        if target.is_null() || target.file != 0 {
                            unimplemented!("mod-only: TrivialAnimationActivationBehaviour with a null or outside AnimationBehaviour");
                        }
                        let phys = *l.arenas.physical.get(&o).unwrap_or_else(|| unimplemented!("mod-only: TrivialAnimationActivationBehaviour without a PhysicalBehaviour (NullReferenceException in Update)"));
                        activations.push((self.visuals.len(), *prefab.index.get(&ObjRef { file: l.file, path_id: target.path_id }).ok_or("AnimationBehaviour outside the prefab")?));
                        Kind::Activation { phys, animation: usize::MAX, heat: 0, time: 0.0 }
                    }
                    "ColorWave" => {
                        let Some(own) = self.sprite_at(l, o) else { continue };
                        Kind::ColorWave {
                            renderer: self.sprite_field(l, v, "Renderer")?,
                            own,
                            gradient: Gradient::read(field(v, "Gradient")?)?,
                            Duration: float(v, "Duration")?,
                            Offset: float(v, "Offset")?,
                            UseSinWave: flag("UseSinWave")?,
                            OffsetVaration: float(v, "OffsetVaration")?,
                            SpeedVaration: float(v, "SpeedVaration")?,
                            Randomisation: float(v, "Randomisation")?,
                            oV: 0.0,
                            sV: 0.0,
                        }
                    }
                    "SpriteGradientAnimation" => Kind::SpriteGradient {
                        gradient: Gradient::read(field(v, "Gradient")?)?,
                        Tint: floats::<4>(field(v, "Tint")?)?,
                        Speed: float(v, "Speed")?,
                        Offset: float(v, "Offset")?,
                        CycleType: field(v, "CycleType")?.i64().ok_or("CycleType")? as i32,
                        ReplayOnEnable: flag("ReplayOnEnable")?,
                        PositionOffsetInfluence: float(v, "PositionOffsetInfluence")?,
                        t: 0.0,
                        sprite: self.sprite_field(l, v, "SpriteRenderer")?,
                        at: if l.arenas.physical.contains_key(&o) { unimplemented!("mod-only: SpriteGradientAnimation on a physical object") } else { anchor_of(o) },
                    },
                    // SortingLayerChild.Awake: OnTransformParentChanged's parentRenderer, then its own renderers.
                    "SortingLayerChild" => {
                        if !*enabled {
                            unimplemented!("mod-only: a disabled SortingLayerChild (OnWillRenderObject)");
                        }
                        let line = prefab.objects[o].components.iter().enumerate().find(|(_, (_, k))| matches!(k, Component::Other(120))).map(|(k, _)| inst.objects[o].components[k]).and_then(|id| self.lines.iter().position(|x| x.id == id));
                        // A ParticleSystemRenderer's draw also calls OnWillRenderObject, which then sorts the sprite and
                        // the line too: particles are not drawn here.
                        if prefab.objects[o].components.iter().any(|(_, k)| matches!(k, Component::Other(199))) && (self.sprite_at(l, o).is_some() || line.is_some()) {
                            unimplemented!("mod-only: SortingLayerChild with a ParticleSystemRenderer beside a SpriteRenderer or LineRenderer");
                        }
                        Kind::SortingChild {
                            transform: inst.objects[o].components[0],
                            sprite: self.sprite_at(l, o),
                            line,
                            parent: inst.objects[o].parent.and_then(|p| self.sprite_at(l, p)),
                            Behaviour: field(v, "Behaviour")?.i64().ok_or("SortingLayerChild.Behaviour")? as i32,
                            Offset: field(v, "Offset")?.i64().ok_or("SortingLayerChild.Offset")? as i32,
                        }
                    }
                    "HologramJitter" => {
                        if !*enabled {
                            unimplemented!("mod-only: a disabled HologramJitter (OnWillRenderObject)");
                        }
                        for x in std::iter::once(o).chain(inst.descendants(o)) {
                            if l.arenas.physical.contains_key(&x) || l.arenas.node.contains_key(&x) || prefab.objects[x].components.iter().any(|(_, k)| matches!(k, Component::Collider2D(_) | Component::Rigidbody2D(_) | Component::Joint2D(_))) {
                                unimplemented!("mod-only: HologramJitter over a subtree with physics");
                            }
                        }
                        let own_text = prefab.objects[o].components.iter().enumerate().find(|(_, (_, k))| matches!(k, Component::Script { class, .. } if class == "TextMeshPro")).and_then(|(k, _)| self.world_texts.iter().position(|t| t.id == inst.objects[o].components[k]));
                        let own = match (self.sprite_at(l, o), own_text) {
                            (Some(s), None) => super::visual::HologramOwn::Sprite(s),
                            (None, Some(t)) => super::visual::HologramOwn::Text(t),
                            (None, None) => continue,
                            (Some(_), Some(_)) => unimplemented!("mod-only: HologramJitter on an object with a SpriteRenderer and a TextMeshPro"),
                        };
                        let mut text_refs = Vec::new();
                        for x in std::iter::once(o).chain(inst.descendants(o)) {
                            for (k, (_, y)) in prefab.objects[x].components.iter().enumerate() {
                                if !matches!(y, Component::Script { class, .. } if class == "TextMeshPro") {
                                    continue;
                                }
                                let Some(i) = self.world_texts.iter().position(|t| t.id == inst.objects[x].components[k]) else { continue };
                                let (mut depth, mut t) = (0, x);
                                while t != o {
                                    t = inst.objects[t].parent.expect("a descendant's parent");
                                    depth += 1;
                                }
                                text_refs.push((i, depth));
                            }
                        }
                        Kind::Hologram {
                            text: self.text_field(l, v, "TextMesh")?,
                            sprite: self.sprite_field(l, v, "Sprite")?,
                            own,
                            Colors: Gradient::read(field(v, "Colors")?)?,
                            Multiply: floats::<4>(field(v, "Multiply")?)?,
                            JitterIntensity: float(v, "JitterIntensity")?,
                            offset: 0.0,
                            local: inst.objects[o].trs,
                            refs: self.subtree_renderers(l, o),
                            text_refs,
                        }
                    }
                    "VertexColourBasedOnTemperatureBehaviour" => Kind::VertexColour {
                        gradient: Gradient::read(field(v, "Gradient")?)?,
                        phys: l.in_arena(field(v, "PhysicalBehaviour")?, &l.arenas.physical, "PhysicalBehaviour")?.unwrap_or_else(|| unimplemented!("mod-only: VertexColourBasedOnTemperatureBehaviour.PhysicalBehaviour is null")),
                        sprite: self.sprite_field(l, v, "SpriteRenderer")?.unwrap_or_else(|| unimplemented!("mod-only: VertexColourBasedOnTemperatureBehaviour.SpriteRenderer is null")),
                        TemperatureRange: floats::<2>(field(v, "TemperatureRange")?)?,
                    },
                    _ => continue,
                };
                out.insert((o, c), self.visuals.len());
                self.visual_ids.insert(id, self.visuals.len());
                self.visuals.push(Visual { id, go: id, enabled: *enabled, kind });
            }
        }
        for (v, at) in activations {
            let Some(&a) = out.get(&at) else { unimplemented!("mod-only: TrivialAnimationActivationBehaviour.AnimationBehaviour is not a Translation, Rotation or ColorAnimationBehaviour") };
            let super::visual::Kind::Activation { animation, .. } = &mut self.visuals[v].kind else { unreachable!() };
            *animation = a;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_assets::HumanRig;

    /// Every dumped field that differs, as "<label> <field>: game <x> ours <y>".
    fn diff(label: &str, mut game: Vec<(&'static str, &mut dyn Field)>, mut ours: Vec<(&'static str, &mut dyn Field)>, out: &mut Vec<String>) {
        for ((name, a), (_, b)) in game.iter_mut().zip(ours.iter_mut()) {
            let (a, b) = (a.show(), b.show());
            if a != b {
                out.push(format!("{label} {name}: game {a} ours {b}"));
            }
        }
    }

    #[test]
    fn an_awake_that_throws_ends_only_itself() {
        let Some((mut g, mut inst, mut prefab, mut db, _)) = super::super::spawn::tests::spawn_recorded("dump31spawnrand") else { return };
        let linked = g.link_instance(&mut db, &prefab, &inst).unwrap();
        let scripts: Vec<(usize, usize)> = (0..prefab.objects.len())
            .flat_map(|o| (0..prefab.objects[o].components.len()).map(move |c| (o, c)))
            .filter(|&(o, c)| matches!(prefab.objects[o].components[c].1, Component::Script { .. }))
            .collect();
        let rename = |prefab: &mut Prefab, (o, c): (usize, usize), name: &str| {
            let Component::Script { class, .. } = &mut prefab.objects[o].components[c].1 else { unreachable!() };
            *class = name.into();
        };
        rename(&mut prefab, scripts[0], "ThrowAtAwake");
        rename(&mut prefab, scripts[1], "ActivateIfTutorial");
        g.prefs.ShowTutorial = true;
        g.awake_behaviours(None, &prefab, &mut inst, &linked, &scripts[..2]);
        assert!(!g.prefs.ShowTutorial, "the next component's Awake ran");
    }

    /// The spawned Person's arenas against dump31spawnrand's first snapshot (after Awake and Start). The links
    /// must match, except those Awake/Start set (LimbBehaviour.NearestLimbToBrain, CirculationBehaviour.Source,
    /// PersonBehaviour.Limbs, the hinges' motors, PhysicalBehaviour's collision mode and grid jitter); dumped
    /// fields are printed where they differ. The recording's poses come from the asset cache, which normalizes
    /// unanimated angles, so only what an unanimated angle uses is compared there.
    #[test]
    fn person_links_match_game() {
        let Some((mut g, inst, prefab, mut db, first)) = super::super::spawn::tests::spawn_recorded("dump31spawnrand") else { return };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let Ok(rig) = HumanRig::load(&root.join("local-cache"), &crate::game_assets::find_game(None).unwrap()) else { return };
        // The map's and pool's splats are not the Person's.
        let mut first = first;
        first.records.retain(|r| r.ty != "FancyBloodSplatController" && r.ty != "BleedingParticleBehaviour");
        let want = Game::from_snapshot(&first, &rig).unwrap();
        let linked = g.link_instance(&mut db, &prefab, &inst).unwrap();
        let person = linked.person.unwrap();
        assert_eq!(g.people[person].BleedingDripSounds, Some(11), "BleedingParticle DripSounds");
        // The queue 0x14 order, as instantiate_native returns it (script execution order, then the higher clone id).
        let mut order: Vec<(usize, usize)> = (0..prefab.objects.len())
            .flat_map(|o| (0..prefab.objects[o].components.len()).map(move |c| (o, c)))
            .filter(|&(o, c)| matches!(prefab.objects[o].components[c].1, Component::Script { .. }))
            .collect();
        order.sort_by_key(|&(o, c)| {
            let Component::Script { execution_order, .. } = &prefab.objects[o].components[c].1 else { unreachable!() };
            (*execution_order, std::cmp::Reverse(inst.objects[o].components[c]))
        });
        let header = super::super::dump::header(&root.join("reference-probe/game/dump31spawnrand.txt")).unwrap();
        g.random = crate::unity::Random { state: super::super::dump::spawn_random_state(&header).unwrap() };
        // spawn_recorded applied the recorded scale; PersonBehaviour.Awake must draw the same one.
        let recorded_scale = inst.objects[0].trs.s;
        let mut inst = inst;
        g.awake_behaviours(None, &prefab, &mut inst, &linked, &order);
        assert_eq!(g.people[person].localScale.map(f32::to_bits), [recorded_scale[0].to_bits(), recorded_scale[1].to_bits(), recorded_scale[2].to_bits()], "PersonBehaviour.Awake scale");
        // Mono's GetHashCode is a heap address: from the recording.
        for p in 0..g.physicals.len() {
            let id = g.physicals[p].id;
            g.physicals[p].hash_code = want.physicals.iter().find(|a| a.id == id).map_or(0, |a| a.hash_code);
        }
        g.queue_starts(Some(&mut db), &prefab, &inst, &linked, &order).unwrap();
        // The clone stays with the game like a real spawn's: Starts that look at the hierarchy find it.
        g.unsynced.push(inst);
        g.run_pending_starts();
        assert!(g.physicals.iter().all(|p| p.in_world));
        // The first frame's render: OnWillRenderObject fills the blood colour caches.
        for l in 0..g.limbs.len() {
            g.skin_will_render(l);
        }
        let mut out = Vec::new();
        let mut links = Vec::new();
        let mut check = |label: String, a: String, b: String| {
            if a != b {
                // Long values: from just before the first difference.
                let at = a.bytes().zip(b.bytes()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len())).saturating_sub(80);
                let cut = |s: &str| s.get(at.min(s.len())..).unwrap_or("").chars().take(240).collect::<String>();
                links.push(format!("{label} @{at}: game {} ours {}", cut(&a), cut(&b)));
            }
        };

        let names = |g: &Game, v: &[usize]| v.iter().map(|&l| g.limbs[l].name.clone()).collect::<Vec<_>>().join(",");
        check("limb names".into(), names(&want, &(0..want.limbs.len()).collect::<Vec<_>>()), names(&g, &(0..g.limbs.len()).collect::<Vec<_>>()));
        for l in 0..g.limbs.len().min(want.limbs.len()) {
            let (a, b) = (&want.limbs[l], &g.limbs[l]);
            let n = b.name.clone();
            check(format!("{n} id"), a.id.to_string(), b.id.to_string());
            check(format!("{n} PhysicalBehaviour"), want.physicals[a.PhysicalBehaviour].id.to_string(), g.physicals[b.PhysicalBehaviour].id.to_string());
            check(format!("{n} Joint"), format!("{:?}", a.Joint), format!("{:?}", b.Joint));
            check(format!("{n} NearestLimbToBrain"), format!("{:?}", a.NearestLimbToBrain), format!("{:?}", b.NearestLimbToBrain));
            check(format!("{n} GripBehaviour"), format!("{:?}", a.GripBehaviour), format!("{:?}", b.GripBehaviour));
            check(format!("{n} ConnectedLimbs"), names(&want, &a.ConnectedLimbs), names(&g, &b.ConnectedLimbs));
            check(format!("{n} StrengthMultipliers"), format!("{:?}", a.StrengthMultipliers), format!("{:?}", b.StrengthMultipliers));
            check(format!("{n} species/blood/decal"), format!("{} {} {}", a.SpeciesIdentity, a.BloodLiquidType, a.BloodDecal), format!("{} {} {}", b.SpeciesIdentity, b.BloodLiquidType, b.BloodDecal.replace(' ', "_")));
            // The recording has none of the fields only the lines' drawing reads.
            let drawn = |g: &GoreString| GoreString { LineWidthMultiplier: 1.0, RenderOrder: 3000, TissueColour: [0.0; 4], ColourSource: None, smoothedColour: [0.0; 4], ..g.clone() };
            check(format!("{n} gore"), format!("{} {} {:?}", a.goreString, a.goreStringJoint, drawn(&a.gore)), format!("{} {} {:?}", b.goreString, b.goreStringJoint, drawn(&b.gore)));
            check(format!("{n} vital"), format!("{:?}", want.vital_parts.get(&l)), format!("{:?}", g.vital_parts.get(&l)));
            let (mut a, mut b) = (a.clone(), b.clone());
            diff(&n, a.fields(), b.fields(), &mut out);
            let (mut a, mut b) = (want.circulations[l].clone(), g.circulations[l].clone());
            check(format!("{n} circulation links"), format!("{:?} {:?} {:?}", a.PushesTo, a.LocalArteryRects, a.Source), format!("{:?} {:?} {:?}", b.PushesTo, b.LocalArteryRects, b.Source));
            diff(&format!("{n} circulation"), a.fields(), b.fields(), &mut out);
            let (mut a, mut b) = (want.nodes[l].clone(), g.nodes[l].clone());
            check(format!("{n} node links"), format!("{:?}", a.Connections), format!("{:?}", b.Connections));
            diff(&format!("{n} node"), a.fields(), b.fields(), &mut out);
            let (mut a, mut b) = (want.skins[l].clone(), g.skins[l].clone());
            check(format!("{n} skin links"), format!("{:?}", a.adjacentLimbs), format!("{:?}", b.adjacentLimbs));
            diff(&format!("{n} skin"), a.fields(), b.fields(), &mut out);
            if let (Some(a), Some(b)) = (want.gforces.get(l), g.gforces.get(l)) {
                let (mut a, mut b) = (a.clone(), b.clone());
                diff(&format!("{n} gforce"), a.fields(), b.fields(), &mut out);
            }
        }
        for (i, b) in g.physicals.iter().enumerate() {
            let Some(a) = want.physicals.iter().find(|a| a.id == b.id) else {
                check(format!("physical {}", b.name), "recorded".into(), b.id.to_string());
                continue;
            };
            let n = format!("{} physical", b.name);
            check(format!("{n} colliders"), format!("{:?} {:?}", a.colliders, a.all_colliders), format!("{:?} {:?}", b.colliders, b.all_colliders));
            check(format!("{n} Properties"), want.properties[a.Properties].name.clone(), g.properties[b.Properties].name.clone());
            check(format!("{n} rigidbody"), a.rigidbody.bodyType.to_string(), b.rigidbody.bodyType.to_string());
            check(format!("{n} transform"), format!("{:?}", a.transform), format!("{:?}", b.transform));
            check(format!("{n} parent"), format!("{:?}", a.parent.map(|t| want.transform_nodes[t].id)), format!("{:?}", b.parent.map(|t| g.transform_nodes[t].id)));
            check(format!("{n} grid count"), format!("{:?}", a.LocalColliderGridPoints.as_ref().map(Vec::len)), format!("{:?}", b.LocalColliderGridPoints.as_ref().map(Vec::len)));
            check(
                format!("{n} misc"),
                format!("{} {} {} {} {} {} {:?}", a.has_fire_map, a.MaximumStabVictims, a.LooseEndSize, a.StabReleaseSpeedModifier, a.StabWoundSizeMultiplier, a.OverrideImpactSounds, a.sprite_bounds),
                format!("{} {} {} {} {} {} {:?}", b.has_fire_map, b.MaximumStabVictims, b.LooseEndSize, b.StabReleaseSpeedModifier, b.StabWoundSizeMultiplier, b.OverrideImpactSounds, b.sprite_bounds),
            );
            let (mut a, mut b) = (a.clone(), g.physicals[i].clone());
            diff(&n, a.fields(), b.fields(), &mut out);
        }
        for b in &g.properties {
            match want.properties.iter().find(|a| a.name == b.name) {
                // Recordings name the ShotImpact asset without a reference to it.
                Some(a) => check(format!("properties {}", a.name), format!("{a:?}"), format!("{:?}", super::physical::PhysicalProperties { ShotImpact: b.ShotImpact.map(|(_, puff)| (None, puff)), ..b.clone() })),
                None => check(format!("properties {}", b.name), "recorded".into(), "missing".into()),
            }
        }
        check("hinge count".into(), want.hinges.len().to_string(), g.hinges.len().to_string());
        for (h, (a, b)) in want.hinges.iter().zip(&g.hinges).enumerate() {
            let show = |gm: &Game, x: &Hinge| format!("{} {:?} {} {:?} {} {} {:?} {}", x.useMotor, x.motor, x.useLimits, x.limits, x.breakForce, x.breakTorque, x.connected.map(|c| gm.limb_of_body(c)), x.enabled);
            check(format!("hinge {h}"), show(&want, a), show(&g, b));
        }
        check("grip count".into(), want.grips.len().to_string(), g.grips.len().to_string());
        for (k, (a, b)) in want.grips.iter().zip(&g.grips).enumerate() {
            check(format!("grip {k}"), format!("{} {:?}", want.physicals[a.PhysicalBehaviour].id, a.CurrentlyHolding), format!("{} {:?}", g.physicals[b.PhysicalBehaviour].id, b.CurrentlyHolding));
            let (mut a, mut b) = (a.clone(), b.clone());
            diff(&format!("grip {k}"), a.fields(), b.fields(), &mut out);
        }
        let node = |g: &Game, t: usize| format!("{} {:?} {:?}", g.transform_nodes[t].id, g.transform_nodes[t].trs, g.transform_nodes[t].parent.map(|p| g.transform_nodes[p].id));
        // The recording lists the map's nodes after the Person's.
        let k = g.transform_nodes.len().min(want.transform_nodes.len());
        check("transform nodes".into(), (0..k).map(|t| node(&want, t)).collect::<Vec<_>>().join(" | "), (0..g.transform_nodes.len()).map(|t| node(&g, t)).collect::<Vec<_>>().join(" | "));
        let (a, b) = (&want.people[0], &g.people[person]);
        check("person".into(), format!("{} {} {} {:?}", a.id, a.BoneBreakClips, a.DismembermentClips, a.localScale), format!("{} {} {} {:?}", b.id, b.BoneBreakClips, b.DismembermentClips, b.localScale));
        check("pose count".into(), a.Poses.len().to_string(), b.Poses.len().to_string());
        check("person runtime links".into(), format!("{} {:?} {:?} {:?}", names(&want, &a.Limbs), a.Head, a.LinkedPoses, a.ActivePose), format!("{} {:?} {:?} {:?}", names(&g, &b.Limbs), b.Head, b.LinkedPoses, b.ActivePose));
        for (pa, pb) in a.Poses.iter().zip(&b.Poses) {
            let show = |p: &RagdollPose| {
                let angles: Vec<String> = p.Angles.iter().map(|x| match x.Animated {
                    true => format!("{} anim {} {} {} {} {} {} {} {} {:?}", x.Limb, x.PoseRigidityModifier, x.StartAngle, x.EndAngle, x.AnimationDuration, x.RandomInfluence, x.RandomSpeed, x.TimeOffset, x.Angle, x.AnimationCurve.as_ref().map(|c| (&c.keys, c.pre, c.post))),
                    false => format!("{} {} {}", x.Limb, x.PoseRigidityModifier, x.Angle),
                }).collect();
                format!("{} {} {} {} {} {} {} {} {} {angles:?}", p.Name, p.Rigidity, p.ForceMultiplier, p.ShouldStandUpright, p.DragInfluence, p.UprightForceMultiplier, p.ShouldStumble, p.State, p.AnimationSpeedMultiplier)
            };
            check(format!("pose {}", pa.Name), show(pa), show(pb));
        }
        let (mut a, mut b) = (a.clone(), b.clone());
        diff("person", a.fields(), b.fields(), &mut out);
        for l in &links {
            eprintln!("LINK {l}");
        }
        for l in &out {
            eprintln!("FIELD {l}");
        }
        assert!(links.is_empty(), "{} link mismatches", links.len());
    }
}

/// The persistent UnityEvent calls of `EventCall` that act on components.
impl Game {
    // PhysicalBehaviour.System.Collections.IEnumerator.get_Current: the waitOneFrame iterator's plumbing.
    // PhysicalBehaviour.System.Collections.Generic.IEnumerator<System.Object>.get_Current: likewise.
    // PhysicalBehaviour.System.Collections.IEnumerator.Reset: likewise (never called).
    // PhysicalBehaviour.System.IDisposable.Dispose: likewise (empty).
    /// PhysicalBehaviour.HaltNextPropagation: `StartCoroutine(waitOneFrame())`, which yields WaitForSeconds(0.016f)
    /// and then sets ActivationPropagationDelay to -1f. Nothing starts on an inactive GameObject.
    /// UnityEvent.Invoke of component `id`'s UnityEvent field `name`: its persistent calls in order.
    pub(crate) fn invoke_event(&mut self, id: i32, name: &str) {
        if let Some(calls) = self.unity_events.get(&(id, name.to_string())).cloned() {
            for c in calls {
                self.event_call(c);
            }
        }
    }

    /// An `AudioCall`; a destroyed target is a null target, which UnityEvent skips.
    pub(super) fn audio_call(&mut self, id: i32, a: AudioCall) {
        use AudioCall::*;
        if self.destroyed_components.contains(&id) {
            return;
        }
        match a {
            PlayClipOnce(file, p) => {
                let clip = self.audio.event_clips.get(&(file, p)).cloned();
                let ph = self.physicals.iter().position(|x| x.id == id).expect("a PhysicalBehaviour in the arena");
                self.play_clip_once(ph, clip, 1.0);
            }
            Toggle => {
                let s = self.audio.source_field(id, "AudioSource").unwrap_or_else(|| super::menu::throw("NullReferenceException: AudioSourceToggleBehaviour.AudioSource"));
                if self.audio.is_playing(s) {
                    self.audio.stop(s);
                } else {
                    self.audio.play(s);
                }
            }
            _ if !self.audio.sources.contains_key(&id) => {}
            Play => self.audio.play(id),
            Stop => self.audio.stop(id),
            Pause => self.audio.pause(id),
            UnPause => self.audio.unpause(id),
            PlayOneShot(file, p) => {
                let clip = self.audio.event_clips.get(&(file, p)).cloned();
                self.audio.play_one_shot(id, clip.as_ref(), 1.0);
            }
            SetEnabled(on) => self.audio.set_enabled(id, on),
            SetMute(on) => self.audio.get_mut(id).mute = on,
        }
    }

    pub(super) fn physical_halt_next_propagation(&mut self, id: i32) {
        let mut waited = false;
        self.start_coroutine(
            id,
            id,
            Box::new(move |g: &mut Game| {
                // PhysicalBehaviour.MoveNext: waitOneFrame's two steps
                if !std::mem::replace(&mut waited, true) {
                    return super::coroutine::Yield::WaitForSeconds(0.016);
                }
                if let Some(p) = g.physicals.iter().position(|p| p.id == id && !p.destroyed) {
                    g.physical_halt_propagation(p);
                }
                super::coroutine::Yield::Done
            }),
        );
    }

    /// Joint2D.set_breakForce on a prefab joint: the live setter of whichever arena entry owns the component.
    pub(super) fn joint_set_break_force(&mut self, id: i32, v: f32) {
        let slot = self.anchored_joints.iter().position(|a| a.component == id).unwrap_or_else(|| unimplemented!("mod-only: Joint2D.set_breakForce on a joint outside the anchored joints"));
        if let Some(&f) = self.adopted_slots.get(&slot) {
            self.fixed_joint_set_break_force(f, v);
        } else if let Some(h) = self.anchored_joints[slot].hinge {
            self.set_hinge_break_force(h, v);
        } else {
            let v = crate::b2::weld::clamp_break(v);
            self.anchored_joints[slot].break_force = v;
            let torque = self.anchored_joints[slot].break_torque;
            self.add_break_limits_update(BreakJoint::Anchored(slot), v, torque);
        }
    }

    /// Rigidbody2D::SetSleeping(false) (Rigidbody2D_CUSTOM_WakeUp 180c55140 -> 180c241d0): a live non-static body is
    /// flagged awake with its sleep time zeroed, whether it slept or not; a static one has awake cleared and its
    /// velocities, force and sleep time zeroed.
    pub(super) fn rigidbody_wake_up(&mut self, id: i32) {
        let owner = self.rigidbody_owner(id).expect("Rigidbody2D.WakeUp: a Rigidbody2D outside the arenas");
        let rb = self.owner_rigidbody(owner);
        if rb.detached.is_some() {
            return;
        }
        self.world.body_mut(rb.body).set_sleeping_false();
    }

    /// Rigidbody2D.AddTorque(float), ForceMode2D.Force.
    pub(super) fn rigidbody_add_torque(&mut self, id: i32, torque: f32) {
        let owner = self.rigidbody_owner(id).expect("Rigidbody2D.AddTorque: a Rigidbody2D outside the arenas");
        let rb = self.owner_rigidbody(owner).clone();
        rb.add_torque(&mut self.world, torque);
    }
}

impl Game {
    /// AddForceBehaviour.AddRelativeForce: `intensity` times |lossyScale.x * lossyScale.y| when ScaleWithSize, then
    /// GetComponent<Rigidbody2D>().AddForce(transform.TransformDirection(LocalAxis) * intensity, ForceMode).
    pub(crate) fn add_force_relative(&mut self, id: i32, mut intensity: f32) {
        let a = self.add_forces.iter().find(|a| a.id == id).expect("an AddForceBehaviour in the arena").clone();
        let p = a.physical;
        if self.physicals[p].destroyed {
            return;
        }
        if a.ScaleWithSize {
            let s = self.lossy_scale(p);
            intensity *= (s[0] * s[1]).abs();
        }
        let dir = mathf::rotate(self.physicals[p].transform.rotation, [a.LocalAxis[0], a.LocalAxis[1], 0.0]);
        let force = [dir[0] * intensity, dir[1] * intensity];
        let rb = self.physicals[p].rigidbody.clone();
        match a.ForceMode {
            0 => rb.add_force(&mut self.world, force),
            1 => rb.add_force_impulse(&mut self.world, force),
            m => unimplemented!("mod-only: AddForceBehaviour with ForceMode2D {m}"),
        }
    }
}

/// The DripSounds length of a BleedingParticle prefab's BleedingParticleBehaviour (CirculationBehaviour copies it) and
/// whether its WaterPuffSystem is set.
pub(crate) fn bleeding_drip_sounds(db: &mut AssetDb, go: ObjRef) -> Result<(usize, bool), String> {
    for c in db.game_object(go)?.components {
        let c = db.resolve(go.file, c).ok_or("a BleedingParticlePrefab component outside the data")?;
        if db.class(c) != 114 {
            continue;
        }
        if let Some((script, _, bv)) = db.mono(c)? {
            if script.class_name == "BleedingParticleBehaviour" {
                return Ok((array(&bv, "DripSounds")?.len(), field(&bv, "WaterPuffSystem")?.pptr().is_some_and(|p| !p.is_null())));
            }
        }
    }
    Err("BleedingParticlePrefab without BleedingParticleBehaviour".into())
}

/// Rigidbody2D::Create's SetInterpolation (180c23700): previous, current and body are the pose Create read (zero
/// for an inactive object's), not interpolating.
fn created_movement(inst: &super::spawn::Instance, o: usize) -> MovementState {
    let pose = inst.created_pose.get(&o).copied().unwrap_or_default();
    MovementState { previous: pose, current: pose, body: pose, ..Default::default() }
}

/// TranslationAnimationBehaviour.Awake: startPos = transform.localPosition.
pub(crate) fn translation_animation_awake(local_position: [f32; 3]) -> [f32; 3] {
    local_position
}
