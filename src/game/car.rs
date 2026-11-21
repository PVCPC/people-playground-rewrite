//! CarBehaviour and the native WheelJoint2D setters it calls.
#![allow(non_snake_case)]

use super::spawn::Instance;
use super::{d, script, BreakJoint, Game};
use crate::assets::components::JointKind as JointData;
use crate::assets::db::{Component, ObjRef};
use crate::b2::weld::clamp_break;
use crate::b2::wheel_joint;
use crate::game_assets::{Curve, CurveCache};

script! {
    CarBehaviour {
        MotorSpeed: f32 = 1200.0,
        WheelJointStrengthMultiplier: f32 = 1.0,
        Activated: bool = false,
        WaterProof: bool = false,
        IsBrakeEngaged: bool = true,
        Health: f32 = 1.0,
        CollideWithChildren: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Phys: the serialized one, else GetComponent<PhysicalBehaviour>().
        Phys: usize = 0,
        /// Phys is on the CarBehaviour's own GameObject.
        own_phys: bool = true,
        /// AccelerationCurve and its native evaluation cache.
        AccelerationCurve: Option<Curve> = None,
        cache: CurveCache = CurveCache::default(),
        time: f32 = 0.0,
        /// WheelJoints: the WheelJoint2D component ids, set by Awake.
        WheelJoints: Vec<i32> = Vec::new(),
        /// Behaviour.enabled
        enabled: bool = true,
        /// EngineLoudness, and the AudioSource Awake adds on audioSourcePosition (its placement; ids set by Awake).
        EngineLoudness: f32 = 1.0,
        engine_proto: super::audio::AudioSource = super::audio::AudioSource::default(),
        engine: i32 = 0,
        engine_single: i32 = 0,
    }
}

script! {
    TruckDoor {
        Activated: bool = false,
    }
    links {
        /// joint: GetComponent<HingeJoint2D>() as a `Game::hinges` index.
        joint: Option<usize> = None,
        /// transform.root's PhysicalBehaviour, else its Transform is the clone's first object.
        root_physical: Option<usize> = None,
    }
}

/// A UnityEvent persistent call on a CarBehaviour: SetHealth(h) or Behaviour.set_enabled(false).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CarCall {
    SetHealth(f32),
    Disable,
    /// A CarBehaviour method the rewrite does not have: it stops when the call runs.
    Stop(&'static str),
}

/// The active objects below `o` (itself included), depth first.
pub(super) fn active_below(inst: &Instance, o: usize, out: &mut Vec<usize>) {
    if !inst.objects[o].active {
        return;
    }
    out.push(o);
    // (A child SetParent(null) took out, a DetachFromParentOnStart object, is a root of its own now.)
    for &child in inst.prefab.objects[o].children.iter().filter(|&&c| inst.objects[c].parent == Some(o)) {
        active_below(inst, child, out);
    }
}

/// GetComponentsInChildren<Collider2D>() of object `o`: its active objects, depth first, each object's colliders in
/// component order, as (object, component id).
pub(super) fn colliders_below(inst: &Instance, o: usize) -> Vec<(usize, i32)> {
    let mut objects = Vec::new();
    active_below(inst, o, &mut objects);
    let mut out = Vec::new();
    for x in objects {
        for (k, (_, c)) in inst.prefab.objects[x].components.iter().enumerate() {
            if matches!(c, Component::Collider2D(_)) {
                out.push((x, inst.objects[x].components[k]));
            }
        }
    }
    out
}

/// JointSerialisationHelper / BatchJointSerialisationHelper: the break limit key of each joint on the helper's own
/// GameObject (the only ones whose OnJointBreak2D message reaches it; None: not one of them) and its Broken flag.
#[derive(Clone, Debug)]
pub struct JointHelper {
    /// The component's instance id.
    pub id: i32,
    /// BatchJointSerialisationHelper (Broken is a bool[]) rather than JointSerialisationHelper (one bool).
    pub batch: bool,
    pub joints: Vec<Option<BreakJoint>>,
    pub Broken: Vec<bool>,
}

/// NoCollideGm: the objects A and B (None: a null reference), by the component's instance id.
#[derive(Clone, Copy, Debug)]
pub struct NoCollideGm {
    pub id: i32,
    pub a: Option<usize>,
    pub b: Option<usize>,
}

/// NoCollideGm's Awake and OnDestroy loops: every collider below A against every collider below B, but itself.
pub(super) fn no_collide_gm_pairs(inst: &Instance, a: usize, b: usize) -> Vec<(i32, i32)> {
    let (below_b, mut out) = (colliders_below(inst, b), Vec::new());
    for (_, x) in colliders_below(inst, a) {
        out.extend(below_b.iter().filter(|&&(_, y)| x != y).map(|&(_, y)| (x, y)));
    }
    out
}

/// The (object, component index) of component `id` in a clone.
fn key_of(inst: &Instance, id: i32) -> (usize, usize) {
    let o = inst.objects.iter().position(|x| x.components.contains(&id)).expect("a component of this clone");
    (o, inst.objects[o].components.iter().position(|&x| x == id).unwrap())
}

/// What a RandomCarTextureBehaviour renderer is in the arenas: the object's physical, or a bare renderer's component id.
#[derive(Clone, Copy, Debug)]
pub enum CarSlot {
    Physical(usize),
    Renderer(i32),
}

/// Default: before Awake fills the renderers (a null component).
impl Default for CarSlot {
    fn default() -> Self {
        CarSlot::Renderer(0)
    }
}

/// RandomCarTextureBehaviour: the six renderers and each texture's six sprites with the bounds the renderer gives.
/// The outline refresh and context menu are UI.
#[derive(Clone, Debug)]
pub struct RandomCarTexture {
    pub id: i32,
    /// Awake's GetComponent<PhysicalBehaviour>()
    pub physical: Option<usize>,
    pub chosenIndex: i32,
    pub slots: [CarSlot; 6],
    pub textures: Vec<[(ObjRef, ([f32; 3], [f32; 3])); 6]>,
}

impl Default for RandomCarTexture {
    fn default() -> Self {
        Self {
            id: Default::default(),
            physical: Default::default(),
            chosenIndex: i32::MIN,
            slots: Default::default(),
            textures: Default::default(),
        }
    }
}

impl Game {
    /// RandomCarTextureBehaviour.Awake
    pub fn random_car_texture_awake(&mut self, t: usize, enabled: bool) {
        if enabled {
            let n = self.random_car_textures[t].textures.len() as i32;
            if n != 0 {
                self.random_car_textures[t].chosenIndex = self.random.range_int(0, n);
            }
            self.random_car_texture_set_to_index(t);
        }
    }

    /// RandomCarTextureBehaviour.Start: SetSpritesToIndex, then the next and previous texture buttons.
    pub fn random_car_texture_start(&mut self, t: usize) {
        use super::menu::{MenuAction as A, MenuButton as B};
        self.random_car_texture_set_to_index(t);
        let p = self.random_car_textures[t].physical.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        let menu = &mut self.physicals[p].menu;
        menu.push(B { LabelWhenMultipleAreSelected: crate::il!(str "RandomCarTextureBehaviour::Start" 1), ..B::new("nextSkin", crate::il!(str "RandomCarTextureBehaviour::Start" 1), crate::il!(str "RandomCarTextureBehaviour::Start" 2), A::CarTexture(t, 1)) });
        menu.push(B { LabelWhenMultipleAreSelected: crate::il!(str "RandomCarTextureBehaviour::Start" 5), ..B::new("previousSkin", crate::il!(str "RandomCarTextureBehaviour::Start" 5), crate::il!(str "RandomCarTextureBehaviour::Start" 6), A::CarTexture(t, -1)) });
    }

    /// RandomCarTextureBehaviour.SetSpritesToIndex
    pub fn random_car_texture_set_to_index(&mut self, t: usize) {
        let x = &self.random_car_textures[t];
        let n = x.textures.len() as i32;
        if n == 0 {
            return;
        }
        let i = if x.chosenIndex == i32::MIN {
            self.random.range_int(0, n)
        } else if x.chosenIndex >= n {
            0
        } else if x.chosenIndex < 0 {
            n - 1
        } else {
            x.chosenIndex
        };
        self.random_car_textures[t].chosenIndex = i;
        let x = self.random_car_textures[t].clone();
        for (slot, (sprite, bounds)) in x.slots.iter().zip(x.textures[i as usize]) {
            match *slot {
                CarSlot::Physical(p) => {
                    let ph = &mut self.physicals[p];
                    ph.sprite.as_mut().expect("RandomCarTextureBehaviour without a SpriteRenderer").0 = sprite;
                    ph.sprite_bounds = Some(bounds);
                }
                CarSlot::Renderer(id) => self.renderers.iter_mut().find(|r| r.id == id).expect("RandomCarTextureBehaviour: a renderer without a sprite").sprite.0 = sprite,
            }
        }
        self.invoke_event(x.id, "OnAfterChange");
    }

    /// AutomaticWheelJointCreator.Awake. Each AddComponent<WheelJoint2D>() is a new component of the prefab copy, so
    /// CarBehaviour.Awake, later on the same GameObject, finds it. WheelJoint2D::Reset (180bf7a50) gives suspension 0.7, 2,
    /// 90 and motor 0, 10000 with useMotor off; Create returns until connectedBody is set; SetAnchor, SetConnectedAnchor and
    /// SetSuspension (180bf8860) each ReCreate the live joint.
    pub fn automatic_wheel_joint_creator_awake(&mut self, inst: &mut Instance, o: usize, wheels: &[crate::assets::classes::PPtr], frequency: f32, dampening: f32) {
        use crate::b2::math::Vec2;
        use crate::b2::weld::FixedJoint2d;
        for w in wheels {
            let id = self.allocate_instance_id();
            let reset = FixedJoint2d::reset();
            let mut prefab = (*inst.prefab).clone();
            let go = prefab.objects[o].game_object;
            let (to, _) = prefab.index[&crate::assets::db::ObjRef { file: go.file, path_id: w.path_id }];
            let Component::Transform(t) = &prefab.objects[to].components[0].1 else { unreachable!() };
            let anchor = Vec2::new(t.local_position[0], t.local_position[1]);
            let c = prefab.objects[o].components.len();
            let kind = JointData::Wheel { damping_ratio: 0.7, frequency: 2.0, angle: 90.0, use_motor: false, motor_speed: 0.0, max_motor_torque: 10000.0 };
            let j = crate::assets::components::Joint2D {
                game_object: crate::assets::classes::PPtr { file: 0, path_id: go.path_id },
                enabled: true,
                enable_collision: reset.enable_collision,
                connected_body: *w,
                break_force: reset.break_force,
                break_torque: reset.break_torque,
                auto_configure_connected_anchor: reset.auto_configure_connected_anchor,
                anchor: [0.0; 2],
                connected_anchor: [0.0; 2],
                kind,
            };
            let r = crate::assets::db::ObjRef { file: go.file, path_id: -(c as i64) - 1 };
            prefab.objects[o].components.push((r, Component::Joint2D(j.clone())));
            let prefab = std::sync::Arc::new(prefab);
            inst.prefab = prefab.clone();
            inst.objects[o].components.push(id);
            let k = (o, c);
            self.anchored_joint2d_create(inst, &prefab, o, c, &j);
            for step in 0..3 {
                self.anchored_joint2d_cleanup(inst, o, c);
                let st = inst.anchored.get_mut(&k).unwrap();
                match step {
                    0 => st.base.anchor = anchor,
                    1 => st.base.connected_anchor = Vec2::new(0.0, 0.0),
                    _ => {
                        let s = st.wheel.as_mut().unwrap();
                        (s.frequency, s.damping_ratio) = (frequency, dampening);
                    }
                }
                self.anchored_joint2d_create(inst, &prefab, o, c, &j);
            }
            self.wheel_set_use_motor(inst, k, true);
        }
    }

    /// CarBehaviour.Awake. The two AudioSources (AddComponent on audioSourcePosition) take instance ids.
    pub fn car_awake(&mut self, car: usize, inst: &Instance, o: usize) {
        self.cars[car].IsBrakeEngaged = true;
        let mut s = self.cars[car].engine_proto.clone();
        s.set_min_distance(self.cars[car].EngineLoudness);
        s.set_max_distance(100.0);
        s.doppler_level = 0.0;
        s.set_spatial_blend(1.0);
        s.play_on_awake = false;
        let single = s.clone();
        s.clip = self.audio.clip(self.cars[car].id, "EngineLoop");
        s.looping = true;
        let (a, b) = (self.allocate_instance_id(), self.allocate_instance_id());
        self.audio.add(a, s);
        self.audio.add(b, single);
        self.cars[car].engine = a;
        self.cars[car].engine_single = b;
        // GetComponentsInChildren<WheelJoint2D>()
        let mut objects = Vec::new();
        active_below(inst, o, &mut objects);
        let mut wheels = Vec::new();
        for x in objects {
            for (k, (_, c)) in inst.prefab.objects[x].components.iter().enumerate() {
                if matches!(c, Component::Joint2D(j) if matches!(j.kind, JointData::Wheel { .. })) {
                    wheels.push(inst.objects[x].components[k]);
                }
            }
        }
        let strength = (15000.0 * d(self.cars[car].WheelJointStrengthMultiplier)).f();
        for &w in &wheels {
            self.wheel_set_break_force(inst, key_of(inst, w), strength);
        }
        self.cars[car].WheelJoints = wheels;
    }

    /// Joint2D::SetBreakForce (180c05980) on a prefab WheelJoint2D: the clamped value is stored, then
    /// AddBreakLimitsUpdate moves the joint to the back of the scene's list.
    fn wheel_set_break_force(&mut self, inst: &Instance, k: (usize, usize), v: f32) {
        let Some(st) = inst.anchored.get(&k) else { unimplemented!("mod-only: CarBehaviour: a WheelJoint2D that was never created") };
        let Some(slot) = st.slot else { unimplemented!("mod-only: CarBehaviour: a WheelJoint2D without a b2 joint") };
        let v = clamp_break(v);
        self.anchored_joints[slot].break_force = v;
        let torque = self.anchored_joints[slot].break_torque;
        self.add_break_limits_update(BreakJoint::Anchored(slot), v, torque);
    }

    /// WheelJoint2D::SetUseMotor (180bf8890): stored, then written to the live joint.
    fn wheel_set_use_motor(&mut self, inst: &mut Instance, k: (usize, usize), v: bool) {
        let st = inst.anchored.get_mut(&k).unwrap();
        st.wheel.as_mut().unwrap().use_motor = v;
        if let Some(j) = st.slot.and_then(|s| self.anchored_joints[s].joint) {
            wheel_joint::enable_motor(&mut self.world, j, v);
        }
    }

    /// WheelJoint2D::SetMotor (180bf8760): the motor is stored with useMotor on; a live joint gets EnableMotor(true),
    /// the speed in radians and the maximum torque.
    fn wheel_set_motor(&mut self, inst: &mut Instance, k: (usize, usize), speed: f32, max_torque: f32) {
        let st = inst.anchored.get_mut(&k).unwrap();
        let w = st.wheel.as_mut().unwrap();
        (w.motor_speed, w.max_motor_torque, w.use_motor) = (speed, max_torque, true);
        if let Some(j) = st.slot.and_then(|s| self.anchored_joints[s].joint) {
            wheel_joint::enable_motor(&mut self.world, j, true);
            wheel_joint::set_motor_speed(&mut self.world, j, speed * crate::game::mathf::DEG2RAD);
            wheel_joint::set_max_motor_torque(&mut self.world, j, max_torque);
        }
    }

    /// JointSerialisationHelper.OnJointBreak2D of the helpers on the broken joint's GameObject.
    pub fn joint_helpers_on_break(&mut self, j: BreakJoint) {
        for h in 0..self.joint_helpers.len() {
            self.joint_helper_on_joint_break(h, j);
        }
    }

    /// JointSerialisationHelper.OnJointBreak2D(joint) of helper `h`: Broken = true when `joint` is its Joint (a
    /// BatchJointSerialisationHelper: each of its Joints that is).
    pub fn joint_helper_on_joint_break(&mut self, h: usize, j: BreakJoint) {
        let h = &mut self.joint_helpers[h];
        for (key, broken) in h.joints.iter().zip(&mut h.Broken) {
            if *key == Some(j) {
                *broken = true;
            }
        }
    }

    /// Object.Destroy(Joint2D) of a joint of the break-limit list, as the break walk's DestroyObjectHighLevel does it but without
    /// the OnJointBreak2D messages (a joint already gone is not in the list: destroying it again does nothing).
    pub(super) fn destroy_break_joint(&mut self, j: BreakJoint) {
        if !self.joint_break_limits.contains(&j) {
            return;
        }
        match j {
            BreakJoint::Hinge(h) => {
                if let Some(id) = self.hinges[h].joint.take() {
                    self.world.destroy_joint(id);
                }
                self.hinges[h].destroyed = true;
            }
            BreakJoint::Spring(l) => self.remove_spring_joint(l),
            BreakJoint::Anchored(a) => {
                if let Some(id) = self.anchored_joints[a].joint.take() {
                    self.world.destroy_joint(id);
                }
                self.destroyed_components.insert(self.anchored_joints[a].component);
            }
            BreakJoint::Fixed(f) => self.fixed_joint_destroy(f),
        }
        self.joint_break_limits.retain(|&k| k != j);
    }

    /// JointSerialisationHelper.OnAfterDeserialise and BatchJointSerialisationHelper.OnAfterDeserialise: Object.Destroy of each
    /// joint whose Broken was saved true.
    pub(crate) fn joint_helper_on_after_deserialise(&mut self, h: usize) {
        let broken: Vec<BreakJoint> = self.joint_helpers[h].joints.iter().zip(&self.joint_helpers[h].Broken).filter(|(_, b)| **b).filter_map(|(k, _)| *k).collect();
        for j in broken {
            self.destroy_break_joint(j);
        }
    }

    /// NoCollideGm.Awake
    pub fn no_collide_gm_awake(&mut self, id: i32, inst: &Instance) {
        let g = *self.no_collide_gms.iter().find(|g| g.id == id).expect("a NoCollideGm");
        let (Some(a), Some(b)) = (g.a, g.b) else { return };
        for (x, y) in no_collide_gm_pairs(inst, a, b) {
            self.ignore_collision_substitute(x, y, true);
        }
    }

    /// NoCollideGm.OnDestroy: the destroyed object's subtree is already inactive.
    pub fn no_collide_gm_destroy(&mut self, id: i32, inst: &Instance) {
        let g = *self.no_collide_gms.iter().find(|g| g.id == id).expect("a NoCollideGm");
        let (Some(a), Some(b)) = (g.a, g.b) else { return };
        for (x, y) in no_collide_gm_pairs(inst, a, b) {
            self.ignore_collision_substitute(x, y, false);
        }
    }

    /// CarBehaviour.Start
    pub fn car_start(&mut self, car: usize) {
        use super::menu::{Label, MenuAction as A, MenuButton as B};
        let menu = &mut self.physicals[self.cars[car].Phys].menu;
        menu.push(B::new("reverseCar", crate::il!(str "CarBehaviour::Start" 1), crate::il!(str "CarBehaviour::Start" 2), A::CarReverse(car)));
        menu.push(B { label: Label::CarBrake(car), ..B::new("toggleBreak", "", crate::il!(str "CarBehaviour::Start" 4), A::CarBrake(car)) });
        menu.push(B { label: Label::CarEngine(car), ..B::new("repairEngine", "", crate::il!(str "CarBehaviour::Start" 6), A::CarEngine(car)) });
        if !self.cars[car].CollideWithChildren {
            let colliders = self.with_component_instance(self.cars[car].id, |_, inst, o| colliders_below(inst, o));
            for &(oa, a) in &colliders {
                for &(ob, b) in &colliders {
                    if oa != ob {
                        self.ignore_collision_substitute(a, b, true);
                    }
                }
            }
        }
        self.car_update_activated(car);
    }

    /// CarBehaviour.Update
    pub fn car_update(&mut self, car: usize) {
        let dt = self.time.deltaTime;
        self.cars[car].time = (d(self.cars[car].time) + dt).f();
        if self.physicals[self.cars[car].Phys].Wetness > 0.0 && !self.cars[car].WaterProof {
            self.cars[car].Health = (d(self.cars[car].Health) - d(dt) * 0.5).f();
            if self.cars[car].Health < 0.0 && self.cars[car].Activated {
                self.cars[car].Activated = false;
            }
        }
        let c = &self.cars[car];
        let wheels = c.WheelJoints.clone();
        let (brake, activated) = (c.IsBrakeEngaged, c.Activated);
        let motor_speed = c.MotorSpeed;
        let charge = self.physicals[c.Phys].Charge();
        let own = c.own_phys.then(|| self.lossy_scale(c.Phys)[0]);
        let mut sum = 0.0f32;
        self.with_component_instance(self.cars[car].id, |g, inst, o| {
            // base.transform.lossyScale.x
            let lossy = own.unwrap_or_else(|| inst.lossy_scale(o)[0]);
            for &w in &wheels {
                if g.destroyed_components.contains(&w) {
                    continue;
                }
                let k = key_of(inst, w);
                g.wheel_set_use_motor(inst, k, brake || activated);
                if brake || activated {
                    let torque = inst.anchored[&k].wheel.unwrap().max_motor_torque;
                    let speed = match activated {
                        true => {
                            let t = (d(g.cars[car].time) * 0.25).f();
                            let c = &mut g.cars[car];
                            let e = c.AccelerationCurve.as_ref().expect("CarBehaviour.AccelerationCurve").evaluate(&mut c.cache, t);
                            let sign = if motor_speed < 0.0 { -50.0 } else { 50.0 };
                            ((d(motor_speed) + d(charge) * sign) * e * lossy).f()
                        }
                        false => (d(0.0) * lossy).f(),
                    };
                    sum += speed;
                    g.wheel_set_motor(inst, k, speed, torque);
                }
            }
        });
        // engineAudioSource.volume = Max(Abs(num) / WheelJoints.Length / Abs(MotorSpeed), 0.2f)
        let c = &self.cars[car];
        let v = super::mathf::max(sum.abs() / c.WheelJoints.len() as f32 / c.MotorSpeed.abs(), 0.2);
        self.audio.get_mut(c.engine).set_volume(v);
    }

    /// CarBehaviour.Use: channel 1 reverses the gear; else the engine toggles .
    pub fn car_use(&mut self, car: usize, channel: i32) {
        let c = &mut self.cars[car];
        if c.Health < 0.0 || !c.enabled {
            return;
        }
        if channel == 1 {
            self.car_reverse_gear(car);
            return;
        }
        let c = &mut self.cars[car];
        c.time = 0.0;
        c.Activated = !c.Activated;
        self.car_update_activated(car);
    }

    /// CarBehaviour.ReverseGear: MotorSpeed = -MotorSpeed.
    pub(crate) fn car_reverse_gear(&mut self, car: usize) {
        let c = &mut self.cars[car];
        c.MotorSpeed = 0.0 - c.MotorSpeed;
    }

    /// CarBehaviour.UpdateActivated: the engine loop plays or stops and the startup or shutoff clip plays.
    pub(crate) fn car_update_activated(&mut self, car: usize) {
        let c = &self.cars[car];
        let (id, engine, single) = (c.id, c.engine, c.engine_single);
        if c.Activated {
            self.audio.play(engine);
            let clip = self.audio.clip(id, "EngineStartup");
            self.audio.play_one_shot(single, clip.as_ref(), 1.0);
        } else {
            self.audio.stop(engine);
            let clip = self.audio.clip(id, "EngineShutoff");
            self.audio.play_one_shot(single, clip.as_ref(), 1.0);
        }
    }

    /// The "repairEngine" button: Health = Health < 0 ? 1 : -1, then UpdateActivated or OnRepair.
    pub(super) fn car_repair_engine(&mut self, car: usize) {
        let c = &mut self.cars[car];
        c.Health = if c.Health < 0.0 { 1.0 } else { -1.0 };
        if c.Health < 0.0 && c.Activated {
            c.Activated = false;
            self.car_update_activated(car);
        } else {
            let id = c.id;
            self.invoke_event(id, "OnRepair");
        }
    }

    /// CarBehaviour.OnEMPHit: Health = -1, and an activated car is deactivated.
    pub fn car_on_emp_hit(&mut self, c: usize) {
        self.cars[c].Health = -1.0;
        if self.cars[c].Activated {
            self.cars[c].Activated = false;
            self.car_update_activated(c);
        }
    }

    /// A persistent call of a UnityEvent that targets the CarBehaviour with component id `id`.
    pub fn car_invoke(&mut self, id: i32, call: CarCall) {
        let car = self.cars.iter().position(|c| c.id == id).expect("a CarBehaviour in the arena");
        match call {
            // CarBehaviour.SetHealth
            CarCall::SetHealth(h) => {
                self.cars[car].Health = h;
                if h < 0.0 && self.cars[car].Activated {
                    self.cars[car].Activated = false;
                    self.car_update_activated(car);
                }
            }
            CarCall::Stop(what) => unimplemented!("mod-only: {what}"),
            // Behaviour.enabled = false: RemoveFromManager with the OnDisable of an active GameObject.
            CarCall::Disable => {
                if !std::mem::replace(&mut self.cars[car].enabled, false) {
                    return;
                }
                self.behaviours.remove(super::Behaviour::Car(car));
                self.with_component_instance(id, |g, inst, o| {
                    if inst.objects[o].active {
                        g.car_on_disable(car, inst);
                    }
                });
            }
        }
    }

    /// TruckDoorBehaviour.Use: Activated = !Activated.
    pub fn truck_door_use(&mut self, t: usize) {
        let s = &mut self.truck_doors[t].1;
        s.Activated = !s.Activated;
    }

    /// TruckDoorBehaviour.FixedUpdate
    pub fn truck_door_fixed_update(&mut self, t: usize, id: i32) {
        let s = &self.truck_doors[t].1;
        let Some(h) = s.joint.filter(|&h| !self.hinges[h].destroyed) else { return };
        let activated = s.Activated;
        let scale = match s.root_physical {
            Some(p) => self.physicals[p].transform.localScale[0],
            None => self.with_component_instance(id, |_, inst, _| inst.objects[0].trs.s[0]),
        };
        let [_, torque] = self.hinges[h].motor;
        let angle = self.hinges[h].joint_angle(&self.world);
        let target = if activated { 0.0 } else { 90.0f32 * scale };
        let speed = super::mathf::delta_angle(angle, target) * 3.0f32;
        self.hinges[h].set_motor(&mut self.world, speed, torque);
    }

    /// CarBehaviour.OnDisable
    pub fn car_on_disable(&mut self, car: usize, inst: &mut Instance) {
        for w in self.cars[car].WheelJoints.clone() {
            if !self.destroyed_components.contains(&w) {
                let k = key_of(inst, w);
                self.wheel_set_use_motor(inst, k, false);
            }
        }
        self.audio.stop(self.cars[car].engine);
    }
}
