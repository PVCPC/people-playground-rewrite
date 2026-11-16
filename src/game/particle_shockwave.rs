//! NukeShockwaveBehaviour (NukeVFX's Shockwave, a presentation clone the particles own): Start's PushAway raycasts,
//! Update's growth and FixedUpdate's SpawnParticles, which Emits one particle at each ray's hit.

use crate::assets::db::{AssetDb, Component, ObjRef, Prefab};
use crate::assets::mono::Value;
use crate::game::Game;
use crate::game_assets::{Curve, CurveCache};
use bevy::math::{Quat, Vec3};

/// The prefab's script, its object's pose under the clone's root and the Renderer's local bounds.
#[derive(Clone, Debug)]
pub(crate) struct Template {
    /// The ParticleSystem's object (the run's chain[0]).
    pub ps: usize,
    position: Vec3,
    rotation: Quat,
    /// The parent's lossy scale and the object's own localScale.
    parent_scale: f32,
    local_scale: [f32; 3],
    /// Renderer's local bounds (center, extents).
    bounds: ([f32; 3], [f32; 3]),
    duration: f32,
    size_multiplier: f32,
    curve: Curve,
    ray_count: i32,
    mask: u32,
    push_away: f32,
    range_fragment: f32,
    smoke: bool,
}

/// A live clone's script state.
#[derive(Clone, Debug)]
pub(crate) struct Shockwave {
    pub t: Template,
    /// The clone's run group, once the particles' update played it; before, its `effect_log` index.
    pub group: u32,
    pub log: Option<usize>,
    /// SpawnParticles' Emit positions not yet given to the run.
    pub pending: Vec<[f32; 2]>,
    /// transform.position, localScale (x, y, z), and the clone's scale.
    position: [f32; 3],
    scale: [f32; 3],
    clone_scale: f32,
    turn: Quat,
    started: bool,
    enabled: bool,
    start_size: f32,
    time: f32,
    progress: f32,
    cache: CurveCache,
}

/// The NukeShockwaveBehaviours of a prefab, as `Template`s (the scripts of inactive objects never run).
pub(crate) fn templates(db: &mut AssetDb, source: ObjRef, prefab: &Prefab) -> Vec<Template> {
    let mut poses: Vec<(Vec3, Quat, f32, bool)> = Vec::new();
    let mut out = Vec::new();
    for ob in prefab.objects.iter() {
        let (mut position, mut rotation, mut scale) = (Vec3::ZERO, Quat::IDENTITY, [1.0f32; 3]);
        if let Some(Component::Transform(t)) = ob.components.first().map(|c| &c.1) {
            (position, rotation, scale) = (Vec3::from(t.local_position), Quat::from_array(t.local_rotation), t.local_scale);
        }
        if ob.parent.is_none() {
            (position, rotation) = (Vec3::ZERO, Quat::IDENTITY);
        }
        let parent = ob.parent.map_or((Vec3::ZERO, Quat::IDENTITY, 1.0, true), |p| poses[p]);
        let world = parent.0 + parent.1 * (position * parent.2);
        poses.push((world, parent.1 * rotation, parent.2 * scale[0], parent.3 && ob.active));
        let Some(fields) = ob.components.iter().find_map(|(_, c)| match c {
            Component::Script { class, fields, enabled, .. } if class == "NukeShockwaveBehaviour" && *enabled => Some(fields),
            _ => None,
        }) else { continue };
        if !(parent.3 && ob.active) {
            continue;
        }
        let f = |n: &str| fields.get(n).and_then(Value::f32).unwrap_or_else(|| panic!("NukeShockwaveBehaviour.{n}"));
        let object = |n: &str| {
            let p = fields.get(n).and_then(Value::pptr).unwrap_or_else(|| panic!("NukeShockwaveBehaviour.{n}"));
            *prefab.index.get(&ObjRef { file: source.file, path_id: p.path_id }).filter(|_| p.file == 0 && !p.is_null()).unwrap_or_else(|| unimplemented!("mod-only: NukeShockwaveBehaviour.{n} null or outside its prefab (NullReferenceException)"))
        };
        let (ro, rc) = object("Renderer");
        let Component::SpriteRenderer(r) = &prefab.objects[ro].components[rc].1 else { unimplemented!("mod-only: NukeShockwaveBehaviour.Renderer is not a SpriteRenderer") };
        let sprite = db.resolve(source.file, r.sprite).unwrap_or_else(|| unimplemented!("mod-only: NukeShockwaveBehaviour.Renderer without a sprite (empty bounds)"));
        let sprite = crate::assets::components::Sprite::read(db.data(sprite)).expect("NukeShockwaveBehaviour.Renderer's sprite");
        if ro != prefab.objects.iter().position(|x| std::ptr::eq(x, ob)).unwrap() {
            unimplemented!("mod-only: NukeShockwaveBehaviour.Renderer on another object (its bounds follow that Transform)");
        }
        let (pso, _) = object("ParticleSystem");
        let mask = fields.get("ShockwaveEffectMask").and_then(|m| m.get("m_Bits")).and_then(Value::i64).expect("ShockwaveEffectMask.m_Bits") as u32;
        out.push(Template {
            ps: pso,
            position: world,
            rotation: parent.1 * rotation,
            parent_scale: parent.2,
            local_scale: scale,
            bounds: crate::game::link::renderer_bounds(&sprite, r),
            duration: f("Duration"),
            size_multiplier: f("SizeMultiplier"),
            curve: crate::game::link::curve(fields.get("GrowingCurve").expect("GrowingCurve")).expect("GrowingCurve"),
            ray_count: fields.get("RayCount").and_then(Value::i64).expect("RayCount") as i32,
            mask,
            push_away: f("PushAwayDistance"),
            range_fragment: f("RangeFragment"),
            smoke: fields.get("DoShockwaveSmoke").and_then(Value::bool).expect("DoShockwaveSmoke"),
        });
    }
    out
}

impl Shockwave {
    pub(crate) fn new(t: Template, group: u32, root: Vec3, turn: Quat, clone_scale: f32) -> Self {
        let p = root + turn * (t.position * clone_scale);
        let scale = t.local_scale;
        Shockwave { group, log: None, pending: Vec::new(), position: p.to_array(), scale, clone_scale, turn: turn * t.rotation, started: false, enabled: true, start_size: 0.0, time: 0.0, progress: 0.0, cache: CurveCache::default(), t }
    }

    pub(crate) fn alive(&self) -> bool {
        !self.started || self.enabled
    }

    /// Renderer.bounds.extents.x: the local bounds through localToWorld (|m00| ex + |m01| ey + |m02| ez).
    fn extents_x(&self) -> f32 {
        let s = self.t.parent_scale * self.clone_scale;
        let m = bevy::math::Mat3::from_quat(self.turn);
        let e = self.t.bounds.1;
        (m.x_axis.x * self.scale[0] * s).abs() * e[0] + (m.y_axis.x * self.scale[1] * s).abs() * e[1] + (m.z_axis.x * self.scale[2] * s).abs() * e[2]
    }

    /// Start, before the clone's first FixedUpdate or Update: startSize, and with DoShockwaveSmoke the PushAways.
    pub(crate) fn start(&mut self, g: &mut Game) {
        if self.started || !self.enabled {
            return;
        }
        self.started = true;
        self.start_size = self.scale[0];
        if self.t.smoke {
            for i in 0..8 {
                let f = 360.0f32 * (i as f32 / 8.0) * (std::f32::consts::PI / 180.0);
                let dir = [super::mathf::cos(f), super::mathf::sin(f)];
                let at = crate::b2::math::Vec2::new(self.position[0], self.position[1]);
                if let Some(h) = g.raycast(at, dir, self.t.push_away, self.t.mask, f32::NEG_INFINITY, f32::INFINITY) {
                    let k = self.t.push_away - h.distance;
                    self.position[0] -= dir[0] * k;
                    self.position[1] -= dir[1] * k;
                }
            }
        }
    }

    /// FixedUpdate: with DoShockwaveSmoke, SpawnParticles: CeilToInt(RayCount * progress) (at least 1) rays outward
    /// from the bounds' edge, an Emit at each hit (`pending`).
    pub(crate) fn fixed_update(&mut self, g: &mut Game) {
        if !self.started || !self.enabled || !self.t.smoke {
            return;
        }
        let range = self.extents_x();
        let num = super::temperature::ceil_to_int(self.t.ray_count as f32 * self.progress).max(1);
        for i in 0..num {
            let f = 360.0f32 * (i as f32 / num as f32) * (std::f32::consts::PI / 180.0);
            let v = [super::mathf::cos(f), super::mathf::sin(f)];
            let k = range - self.t.range_fragment;
            let at = crate::b2::math::Vec2::new(self.position[0] + v[0] * k, self.position[1] + v[1] * k);
            if let Some(h) = g.raycast(at, v, self.t.range_fragment, self.t.mask, f32::NEG_INFINITY, f32::INFINITY) {
                self.pending.push([h.point.x, h.point.y]);
            }
        }
    }

    /// Update: t += deltaTime; localScale = GrowingCurve(t / Duration) * SizeMultiplier + startSize (z 0); disabled
    /// once t passes Duration.
    pub(crate) fn update(&mut self, dt: f32) {
        if !self.started || !self.enabled {
            return;
        }
        self.time += dt;
        let time = self.time / self.t.duration;
        let num = self.t.curve.evaluate(&mut self.cache, time) * self.t.size_multiplier + self.start_size;
        self.scale = [num, num, 0.0];
        self.progress = time;
        if self.time > self.t.duration {
            self.enabled = false;
        }
    }
}
