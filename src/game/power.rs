//! AOEPowerTool, PushPowerTool, PullPowerTool, LiftPowerTool. A tool stays on the ToolController once added, so its
//! FixedUpdate and its effect keep running after it is unchosen.

use super::mathf;
use super::{d, Game, V};
use crate::assets::db::ObjRef;

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum PowerKind {
    #[default]
    Push,
    Pull,
    Lift,
}

impl PowerKind {
    pub fn of_tool(name: &str) -> Option<Self> {
        Some(match name {
            "PushPowerTool" => Self::Push,
            "PullPowerTool" => Self::Pull,
            "LiftPowerTool" => Self::Lift,
            _ => return None,
        })
    }

    pub(crate) fn effect(self) -> &'static str {
        match self {
            Self::Push => crate::il!(str "PushPowerTool::CreateEffectObject" 0),
            Self::Pull => crate::il!(str "PullPowerTool::CreateEffectObject" 0),
            Self::Lift => crate::il!(str "LiftPowerTool::CreateEffectObject" 0),
        }
    }
}

pub(crate) const DAMPENING_RADIUS: f32 = 5.0;
pub(crate) const RADIUS: f32 = 15.0;
pub(crate) const FORCE: f32 = 550.0;
pub(crate) const MAX_FORCE: f32 = 150.0;
pub(crate) const MOUSE_MOVE_INFLUENCE: f32 = 45.0;
/// LayerMask.GetMask("Debris", "Objects", "CollidingDebris")
const LAYERS: u32 = (1 << 9) | (1 << 10) | (1 << 13);

#[derive(Debug, Default)]
pub struct Power {
    pub kind: PowerKind,
    pub(crate) mouseMovement: [f32; 2],
    oldMousePos: [f32; 2],
    effect: ObjRef,
    /// The effect's clone root.
    pub root: i32,
    /// ParticleSystem.isPlaying: the prefab plays on awake.
    pub playing: bool,
}

impl Game {
    /// SetTool: GetComponents(type), or AddComponent, whose Awake instantiates the effect (power_awake).
    /// AOEPowerTool.OnToolChosen, OnToolUnchosen and OnHold are empty.
    pub(super) fn power_choose(&mut self, kind: PowerKind) {
        let found = self.powers.iter().position(|p| p.kind == kind);
        self.power = Some(found.unwrap_or_else(|| {
            self.allocate_instance_id();
            self.power_awake(kind)
        }));
    }

    /// AOEPowerTool.Awake: effectObject = CreateEffectObject() (its ParticleSystem plays on awake); the layer mask is LAYERS.
    /// Awake again on the same tool makes a new effect and keeps only that one (the old stays in the scene).
    pub(super) fn power_awake(&mut self, kind: PowerKind) -> usize {
        let (effect, root) = self.power_create_effect_object(kind);
        match self.powers.iter().position(|p| p.kind == kind) {
            Some(i) => {
                (self.powers[i].effect, self.powers[i].root, self.powers[i].playing) = (effect, root, true);
                i
            }
            None => {
                self.powers.push(Power { kind, mouseMovement: [0.0; 2], oldMousePos: [0.0; 2], effect, root, playing: true });
                self.powers.len() - 1
            }
        }
    }

    /// PushPowerTool.CreateEffectObject, PullPowerTool.CreateEffectObject, LiftPowerTool.CreateEffectObject and AOEPowerTool.CreateEffectObject:
    /// Object.Instantiate(Resources.Load<GameObject>(the kind's effect)). Returns the prefab and the clone root's Transform id.
    /// The base's null is never reached: every concrete AOEPowerTool overrides it.
    pub(crate) fn power_create_effect_object(&mut self, kind: PowerKind) -> (ObjRef, i32) {
        let db = self.assets.take().expect("Resources.Load without the game's data");
        let effect = db.resource(kind.effect(), 1).ok().flatten().unwrap_or_else(|| panic!("Resources.Load({}) found no GameObject", kind.effect()));
        self.assets = Some(db);
        (effect, self.spawn_prefab(effect, [0.0; 3]))
    }

    /// AOEPowerTool.FixedUpdate, for every tool added.
    pub fn powers_fixed_update(&mut self) {
        let mouse = [self.MousePosition[0], self.MousePosition[1]];
        for p in &mut self.powers {
            let delta = [mouse[0] - p.oldMousePos[0], mouse[1] - p.oldMousePos[1]];
            p.mouseMovement = delta.map(|v| MOUSE_MOVE_INFLUENCE * v);
            p.oldMousePos = mouse;
        }
    }

    // AOEPowerTool.OnToolChosen: empty.
    // AOEPowerTool.OnToolUnchosen: empty.
    // AOEPowerTool.OnHold: empty.
    /// AOEPowerTool.OnSelect
    pub(crate) fn power_on_select(&mut self, power: usize) {
        if !self.powers[power].playing {
            self.powers[power].playing = true;
            let (effect, root) = (self.powers[power].effect, self.powers[power].root);
            self.log_effect(effect, self.MousePosition, [0.0, 0.0, 0.0, 1.0], 1.0, None, Some(root));
        }
    }

    /// AOEPowerTool.OnDeselect
    pub(crate) fn power_on_deselect(&mut self, power: usize) {
        self.powers[power].playing = false;
    }

    /// AOEPowerTool.OnFixedHold
    pub(crate) fn power_on_fixed_hold(&mut self, power: usize) {
        if self.input.ui_block || self.ui_lock() {
            return;
        }
        let center = V::new(self.MousePosition[0], self.MousePosition[1]);
        let mut hits = self.overlap_circle(center, RADIUS, LAYERS);
        hits.truncate(256);
        for (physical, _) in hits {
            if let Some(physical) = physical {
                self.power_handle_object(power, physical);
            }
        }
    }

    /// AOEPowerTool.GetFalloff
    pub(crate) fn power_falloff(mouse_movement: [f32; 2], intensity: f32, sqr_distance: f32, max: f32) -> f32 {
        mathf::min(max, (d(4.0) * intensity / sqr_distance + d(mathf::sqr_magnitude(mouse_movement)) * 0.05f32).f())
    }

    /// PushPowerTool.HandleObject, PullPowerTool.HandleObject, LiftPowerTool.HandleObject
    pub(crate) fn power_handle_object(&mut self, power: usize, physical: usize) {
        let (kind, movement) = (self.powers[power].kind, self.powers[power].mouseMovement);
        let position = self.physicals[physical].transform.position;
        let vector = [self.MousePosition[0] - position[0], self.MousePosition[1] - position[1]];
        let rb = self.physicals[physical].rigidbody.clone();
        let sqr = mathf::sqr_magnitude(vector);
        let direction = match kind {
            PowerKind::Lift => [0.0, 1.0],
            PowerKind::Push | PowerKind::Pull => {
                let num = mathf::max(0.25, mathf::magnitude2(vector));
                if num < DAMPENING_RADIUS {
                    let keep = if kind == PowerKind::Push { 0.95 } else { 0.92 };
                    let v = rb.velocity(&self.world);
                    rb.set_velocity(&mut self.world, v.map(|x| x * keep));
                    let w = rb.angular_velocity(&self.world);
                    rb.set_angular_velocity(&mut self.world, w * keep);
                }
                let sign = if kind == PowerKind::Push { -1.0 } else { 1.0 };
                vector.map(|x| sign * x / num)
            }
        };
        let (pull, follow) = (Self::power_falloff(movement, FORCE, sqr, MAX_FORCE), Self::power_falloff(movement, 1.0, sqr, 1.0));
        let mass = rb.mass(&self.world);
        let force: [f32; 2] = std::array::from_fn(|i| (pull * direction[i] + follow * movement[i]) * mass);
        rb.add_force(&mut self.world, force);
    }
}
