//! ActOnCollide and PointToVelocityBehaviour.
#![allow(non_snake_case)]

use super::physical::get_average_impulse_remove_outliers;
use super::coroutine::{Routine, Yield};
use super::{d, mathf, script, Game};
use crate::b2::contacts2d::ScriptCollision;

script! {
    ActOnCollide {
        ImpactForceThreshold: f32 = 0.0,
        DispatchChance: f32 = 1.0,
    }
    links {
        /// Collider: the Collider2D's instance id; None allows any.
        Collider: Option<i32> = None,
        /// GetComponent<PhysicalBehaviour>() (the collision messages reach the object's physical).
        physical: Option<usize> = None,
        enabled: bool = true,
        /// The component's instance id.
        id: i32 = 0,
        /// Actions: what it calls, in order.
        targets: Vec<ActTarget> = Vec::new(),
        after_physical: bool = true,
        /// The component's index on its GameObject.
        index: usize = 0,
    }
}

script! {
    Inverter {
        Activated: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// PhysicalBehaviour
        physical: usize = 0,
        /// The serialised PhysicalBehaviour is null (FixedUpdate dereferences it).
        physical_null: bool = false,
    }
}

script! {
    PointToVelocity {
        intensity: f32 = 1.0,
        direction: [f32; 2] = [1.0, 0.0],
    }
    links {
        /// The physical whose Rigidbody2D is GetComponent<Rigidbody2D>().
        physical: usize = 0,
        /// The component's instance id.
        id: i32 = 0,
    }
}

script! {
    AndroidSparkCreator {
    }
    links {
        /// GetComponent<LimbBehaviour>() (the limb's index).
        limb: usize = 0,
        /// spark: the GameObject each spark Instantiate copies.
        spark: Option<crate::assets::db::ObjRef> = None,
    }
}

/// EggBehaviour (a key of the keypad on Abyss): its Number and the sprite's local bounds (center, extents).
#[derive(Clone, Debug)]
pub struct Egg {
    pub id: i32,
    pub Number: i32,
    pub bounds: ([f32; 3], [f32; 3]),
    /// TextMesh (world_text.rs).
    pub TextMesh: Option<usize>,
}

// OffsetAlternateBehaviour..ctor: the MonoBehaviour's, with no field initialisers (Source is serialised); Invalid downloads from the developer's server and is not written.
/// OffsetGridBehaviour.Result (E, the error, needs the web server).
#[derive(Clone, Copy)]
enum OffsetGridResult {
    I,
    V,
    C,
}

/// OffsetGridBehaviour (Abyss): Blip's SpriteRenderer component id, and the timer.
#[derive(Clone, Debug)]
pub struct OffsetGrid {
    /// The component's instance id.
    pub id: i32,
    pub Blip: i32,
    pub timer: f32,
}

/// OnlyExistWith: Other and AlsoDelete are joints in the prefabs that carry it (the Bicycle's).
#[derive(Clone, Debug)]
pub struct OnlyExistWith {
    pub id: i32,
    pub Other: i32,
    pub AlsoDelete: i32,
}

/// ActOnDisintegrate: Event runs on its PhysicalBehaviour's OnDisintegration.
#[derive(Clone, Debug)]
pub struct ActOnDisintegrate {
    pub id: i32,
    pub physical: usize,
    pub enabled: bool,
    pub calls: Vec<super::link::EventCall>,
}

impl Game {
    /// AndroidSparkCreator.FixedUpdate: Random.value is only drawn on a limb below a tenth of its health.
    pub fn android_spark_fixed_update(&mut self, i: usize) {
        // AndroidSparkCreator.Awake: limb = GetComponent<LimbBehaviour>() is the linked `limb`
        let l = self.android_sparks[i].limb;
        if self.limbs[l].Health < self.limbs[l].InitialHealth * 0.1 && self.random.value() > 0.9995 {
            let spark = self.android_sparks[i].spark.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
            // Object.Instantiate(spark, transform.position, Quaternion.identity): the limb object's position.
            let position = self.physicals[self.limbs[l].PhysicalBehaviour].transform.position;
            self.spawn_prefab(spark, position);
        }
    }
}

/// A persistent call of ActOnCollide.Actions.
#[derive(Clone, Copy, Debug)]
pub enum ActTarget {
    /// Any other persistent call (`link::audio_call`, `unity_event::call`).
    Audio(super::link::EventCall),
    /// ExplosiveBehaviour.Activate (`Game::explosives`)
    Activate(usize),
    /// FirearmBehaviour.Shoot (`Game::firearms`)
    Shoot(usize),
    /// ProjectileLauncherBehaviour.Launch (`Game::launchers`)
    Launch(usize),
    /// DamagableMachineryBehaviour.ForceBreak (`Game::machinery`)
    ForceBreak(usize),
    /// DamagableMachineryBehaviour.OnEMPHit (`Game::machinery`)
    MachineryEmp(usize),
    /// Behaviour.set_enabled(false) on the ActOnShot with this component id.
    DisableActOnShot(i32),
    /// BlasterBehaviour.Shoot (`Game::blasters`)
    Blast(usize),
    /// BeamformerBehaviour.Shoot (`Game::beamformers`)
    Beam(usize),
    /// GenericScifiWeapon40Behaviour.Fire (`Game::scifi40s`)
    Fire(usize),
    /// PhysicalBehaviour.ForceSendUse (`Game::physicals`)
    ForceSendUse(usize),
    /// JavelinMissileBehaviour.Explode (`Game::javelin_missiles`)
    Explode(usize),
    /// A CarBehaviour call (SetHealth or set_enabled(false)) on the car with component id `.0` (car.rs car_invoke).
    Car(i32, super::car::CarCall),
    /// PhysicalBehaviour.Ignite(bool ignoreFlammability) (`Game::physicals`): ignite(p, ignore) in physical.rs.
    Ignite(usize, bool),
}

script! {
    ActOnShot {
        ShotDamageThreshold: f32 = 0.0,
        CartridgeDamageThreshold: f32 = 0.0,
        Chance: f32 = 1.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// Actions: what it calls, in order.
        targets: Vec<ActTarget> = Vec::new(),
    }
}

/// A script that takes a physical object's collision messages besides the PhysicalBehaviour and LimbBehaviour.
#[derive(Clone, Copy, Debug)]
pub enum CollisionScript {
    Destroyable(usize),
    Explosive(usize),
    ActOnCollide(usize),
    Sticky(usize),
    Bulb(usize),
    Soap(usize),
    PowerHammer(usize),
    /// BalloonBehaviour: OnCollisionStay2D only.
    Balloon(usize),
    /// TorchBehaviour: OnCollisionEnter2D and OnCollisionExit2D.
    Torch(usize),
    /// TelevisionBehaviour: OnCollisionEnter2D (television.rs).
    Television(usize),
    /// JavelinMissileBehaviour: OnCollisionEnter2D (javelin.rs).
    JavelinMissile(usize),
    /// WrenchBehaviour: OnCollisionEnter2D only (wrench.rs).
    Wrench(usize),
    /// NoCollideWithBodyPart: OnCollisionEnter2D only (no_collide.rs).
    NoCollideBodyPart(usize),
    /// PlaneBehaviour and WingBehaviour: OnCollisionEnter2D only (old_weapons.rs).
    Plane(usize),
    Wing(usize),
    Translated(super::translated::TBehaviour),
    /// CrossbowBehaviour: OnCollisionEnter2D only (1.28.45, v45_new.rs).
    Crossbow(usize),
}

impl Game {
    pub(super) fn collision_enter_script(&mut self, s: CollisionScript, c: &ScriptCollision) {
        match s {
            CollisionScript::Destroyable(x) => self.destroyable_on_collision(x, c),
            CollisionScript::Explosive(e) => self.explosive_on_collision_enter(e, c),
            CollisionScript::ActOnCollide(a) => self.act_on_collide_enter(a, c),
            CollisionScript::Sticky(s) => self.sticky_grenade_collision_enter(s, c),
            CollisionScript::Bulb(b) => self.bulb_on_collision_enter(b, c),
            CollisionScript::Soap(s) => self.soap_on_collision_enter(s, c),
            CollisionScript::PowerHammer(h) => self.power_hammer_on_collision_enter(h, c),
            CollisionScript::Balloon(_) => {}
            CollisionScript::Torch(t) => self.torch_on_collision_enter(t, c),
            CollisionScript::Television(t) => self.television_on_collision_enter(t, c),
            CollisionScript::JavelinMissile(m) => self.javelin_missile_on_collision_enter(m, c),
            CollisionScript::Wrench(w) => self.wrench_on_collision_enter(w, c),
            CollisionScript::NoCollideBodyPart(n) => self.no_collide_body_part_enter(n, c),
            CollisionScript::Plane(x) => self.plane_on_collision_enter(x, c),
            CollisionScript::Wing(x) => self.wing_on_collision_enter(x, c),
            CollisionScript::Translated(t) => self.tr_on_collision_enter2d(t, c),
            CollisionScript::Crossbow(x) => self.crossbow_on_collision_enter(x, c),
        }
    }

    pub(super) fn collision_stay_script(&mut self, s: CollisionScript, c: &ScriptCollision) {
        match s {
            CollisionScript::Destroyable(x) => self.destroyable_on_collision(x, c),
            CollisionScript::Soap(s) => self.soap_on_collision_stay(s, c),
            CollisionScript::Balloon(b) => self.balloon_on_collision_stay(b, c),
            CollisionScript::Translated(t) => self.tr_on_collision_stay2d(t, c),
            _ => {}
        }
    }

    /// OnCollisionExit2D: of the scripts beside a PhysicalBehaviour, only translated ones take it.
    pub(super) fn collision_exit_script(&mut self, s: CollisionScript, c: &ScriptCollision) {
        if let CollisionScript::Translated(t) = s {
            self.tr_on_collision_exit2d(t, c);
        }
        if let CollisionScript::Torch(t) = s {
            self.torch_on_collision_exit(t, c);
        }
    }

    /// ActOnCollide.OnCollisionEnter2D: GetContacts fills the 4-entry buffer.
    pub(crate) fn act_on_collide_enter(&mut self, a: usize, c: &ScriptCollision) {
        let s = &self.act_on_collides[a];
        if !s.enabled || self.random.value() > s.DispatchChance {
            return;
        }
        let mut buffer = self.physical_contact_buffer;
        let contacts = c.contacts.len().min(ACT_ON_COLLIDE_BUFFER);
        buffer[..contacts].copy_from_slice(&c.contacts[..contacts]);
        let s = &self.act_on_collides[a];
        if s.Collider.is_none_or(|x| c.other_collider == x) && get_average_impulse_remove_outliers(&buffer, contacts) > s.ImpactForceThreshold {
            for t in s.targets.clone() {
                self.run_act_target(t);
            }
        }
    }

    /// One persistent call of an Actions event.
    pub(super) fn run_act_target(&mut self, t: ActTarget) {
        match t {
            ActTarget::Activate(e) => self.explosive_activate(e),
            ActTarget::Shoot(f) => self.firearm_shoot(f),
            ActTarget::Audio(a) => self.event_call(a),
            ActTarget::Car(id, c) => self.car_invoke(id, c),
            ActTarget::Ignite(p, ignore) => self.ignite(p, ignore),
            ActTarget::Launch(l) => self.launcher_launch(l),
            ActTarget::ForceBreak(m) => self.machinery_force_break(m),
            ActTarget::MachineryEmp(m) => self.machinery_on_emp_hit(m),
            ActTarget::DisableActOnShot(id) => self.act_on_shots.iter_mut().find(|a| a.id == id).expect("the ActOnShot").enabled = false,
            ActTarget::Blast(b) => self.blaster_shoot(b),
            ActTarget::Beam(b) => self.beamformer_shoot(b),
            ActTarget::Fire(f) => self.scifi40_fire(f),
            ActTarget::ForceSendUse(p) => self.physical_force_send_use(p),
            ActTarget::Explode(m) => self.javelin_missile_explode(m),
        }
    }

    /// ActOnShot.Shot: `base.enabled && damage >= ShotDamageThreshold && (!cartridge || cartridge.Damage >=
    /// CartridgeDamageThreshold) && Random.value <= Chance`, then the Actions.
    pub(super) fn act_on_shot_shot(&mut self, a: usize, damage: f32, cartridge: Option<super::ballistics::Cartridge>) {
        let s = &self.act_on_shots[a];
        if !s.enabled || !(damage >= s.ShotDamageThreshold) {
            return;
        }
        if !cartridge.is_none_or(|c| c.Damage >= s.CartridgeDamageThreshold) {
            return;
        }
        if self.random.value() <= self.act_on_shots[a].Chance {
            for t in self.act_on_shots[a].targets.clone() {
                self.run_act_target(t);
            }
        }
    }

    /// PointToVelocityBehaviour.Awake: rb = GetComponent<Rigidbody2D>(), which linking resolves (`physical`'s rigidbody).
    pub fn point_to_velocity_awake(&mut self, _i: usize) {}

    /// InverterBehaviour.Use (a private Use() that SendMessage reaches all the same): Activated = !Activated.
    pub fn inverter_use(&mut self, i: usize) {
        self.inverters[i].Activated = !self.inverters[i].Activated;
    }

    /// InverterBehaviour.FixedUpdate: PhysicalBehaviour.EnergyWireResistance = Activated ? -1 : 1.
    pub fn inverter_fixed_update(&mut self, i: usize) {
        if self.inverters[i].physical_null {
            crate::game::menu::npe();
        }
        let p = self.inverters[i].physical;
        self.physicals[p].EnergyWireResistance = if self.inverters[i].Activated { -1.0 } else { 1.0 };
    }

    /// PointToVelocityBehaviour.FixedUpdate
    pub fn point_to_velocity_fixed_update(&mut self, i: usize) {
        let s = &self.point_to_velocities[i];
        let p = s.physical;
        let rb = self.physicals[p].rigidbody.clone();
        if rb.bodyType != 0 {
            return;
        }
        let vector = if self.physicals[p].parent.is_some() {
            let v = self.transform_vector_chain(p, s.direction);
            [v[0], v[1], 0.0]
        } else {
            self.transform_vector(p, s.direction)
        };
        let velocity = rb.velocity(&self.world);
        let intensity = s.intensity;
        let atan2_degrees = |y: f32, x: f32| (d(crate::game::mathf::atan2(y, x)) * 57.29578f32).f();
        let current = atan2_degrees(vector[1], vector[0]);
        let target = atan2_degrees(velocity[1], velocity[0]);
        let num = mathf::delta_angle(current, target);
        if num.abs() < 25.0 {
            let av = rb.angular_velocity(&self.world);
            rb.set_angular_velocity(&mut self.world, (d(av) * mathf::clamp(num / 25.0, 0.95, 1.0)).f());
        }
        rb.add_torque(&mut self.world, (d(num) * intensity * mathf::magnitude2(velocity)).f());
    }
}

/// NoCollide: the Collider2D instance ids of its two sets (None for a null entry).
#[derive(Default, Clone, Debug)]
pub struct NoCollide {
    pub id: i32,
    pub a: Vec<Option<i32>>,
    pub b: Vec<Option<i32>>,
    pub bis_a: bool,
}

impl Game {
    /// NoCollide.Start
    pub fn no_collide_start(&mut self, id: i32) {
        let n = self.no_collides.iter().find(|n| n.id == id).expect("a NoCollide").clone();
        let other = if n.bis_a { &n.a } else { &n.b };
        for &a in n.a.iter().flatten() {
            for &b in other.iter().flatten() {
                if a != b {
                    self.ignore_collision_substitute(a, b, true);
                }
            }
        }
    }

    /// NoCollide.OnDestroy
    pub fn no_collide_destroy(&mut self, id: i32) {
        let n = self.no_collides.iter().find(|n| n.id == id).expect("a NoCollide").clone();
        for &a in n.a.iter().flatten() {
            for &b in n.b.iter().flatten() {
                self.ignore_collision_substitute(a, b, false);
            }
        }
    }
}

script! {
    StickyGrenade {
        LightInterval: f32 = 1.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// light (None: Blink throws).
        light: Option<super::visual::SpriteAt> = None,
        ArmedLightColour: [f32; 4] = [0.0; 4],
        HotLightColor: [f32; 4] = [0.0; 4],
        /// aboutToExplode
        aboutToExplode: bool = false,
        physical: Option<usize> = None,
        /// Explosive: `Game::explosives` index.
        explosive: usize = 0,
        /// unstickyPropertySet: `Game::properties` indices.
        unsticky: Vec<usize> = Vec::new(),
        after_physical: bool = true,
        /// The component's index on its GameObject.
        index: usize = 0,
        /// isStuck (the C# name: fn-check loads and compares it by name)
        isStuck: bool = false,
        /// joint: the `Game::fixed_joints` entry of the FixedJoint2D CreateJoint added (isStuckToHuman and
        /// aboutToExplode only reach the EMP_STICKY_GRENADE achievement).
        joint: Option<usize> = None,
    }
}

// StickyGrenadeBehaviour.Awake: unstickyPropertySet is `unsticky` (linked from StickyPropertiesToIgnore), and the two layers are the constants below
// StickyGrenadeBehaviour.StartBlinking: sticky_start_blinking (StartCoroutine(Blink()), run by ExplosiveBehaviour.OnArm's EventCall::StickyBlink)
// StickyGrenadeBehaviour.Blink: sticky_start_blinking (the routine: armed colour and 12 toggles 0.474 s apart, hot colour and aboutToExplode, 14 toggles 0.061 s apart; isStuckToHuman and the EMP_STICKY_GRENADE unlock are Steam only)
/// Layer indices of TagManager.asset.
const OBJECTS_LAYER: i32 = 9;
const DEBRIS_LAYER: i32 = 10;

impl Game {
    /// StickyGrenadeBehaviour.StartBlinking: StartCoroutine(Blink()).
    pub fn sticky_start_blinking(&mut self, s: usize) {
        let id = self.sticky_grenades[s].id;
        let mut k = 0u32;
        self.start_coroutine(
            id,
            id,
            Box::new(move |g: &mut Game| g.sticky_blink_step(s, &mut k)),
        );
    }

    /// One MoveNext of StickyGrenadeBehaviour.Blink; `k` counts the toggles done.
    pub(crate) fn sticky_blink_step(&mut self, s: usize, k: &mut u32) -> Yield {
        if *k == 26 {
            return Yield::Done;
        }
        let x = &self.sticky_grenades[s];
        let light = x.light.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        let (armed, hot) = (x.ArmedLightColour, x.HotLightColor);
        if *k == 0 {
            // light.color = ArmedLightColour
            self.set_sprite_color(light, armed);
        } else if *k == 12 {
            // light.color = HotLightColor; aboutToExplode = true
            self.set_sprite_color(light, hot);
            self.sticky_grenades[s].aboutToExplode = true;
        }
        // light.enabled = !light.enabled
        let on = self.sprite_enabled(light);
        self.set_sprite_enabled(light, !on);
        *k += 1;
        Yield::WaitForSeconds(if *k <= 12 { 0.474 } else { 0.061 })
    }

    /// `(bool)joint`: the component exists.
    fn sticky_joint_alive(&self, s: usize) -> bool {
        self.sticky_grenades[s].joint.is_some_and(|f| !self.fixed_joints[f].destroyed)
    }

    /// GameObject.layer = layer on the grenade's object (GameObject::SetLayer, 180589640): kLayerChanged reaches its
    /// components in order, and only Collider2D (RecalculateContacts, 180c14640) and Rigidbody2D (180c1cdc0, a wake)
    /// act on it among the physics ones (a Renderer's is rendering).
    fn sticky_set_layer(&mut self, s: usize, layer: i32) {
        let p = self.sticky_grenades[s].physical.expect("a StickyGrenadeBehaviour receiving collisions has a PhysicalBehaviour");
        let id = self.physicals[p].id;
        self.with_component_instance(id, |g, inst, o| g.game_object_set_layer(inst, o, layer));
    }

    /// StickyGrenadeBehaviour.CreateJoint
    pub(crate) fn sticky_create_joint(&mut self, s: usize, phys: usize) {
        if self.sticky_joint_alive(s) {
            self.destroy_joint_delayed(self.sticky_grenades[s].joint.unwrap());
        }
        let p = self.sticky_grenades[s].physical.expect("a StickyGrenadeBehaviour receiving collisions has a PhysicalBehaviour");
        // AddComponent<FixedJoint2D> takes the component's instance id.
        self.allocate_instance_id();
        let f = self.add_fixed_joint_component(p);
        self.sticky_grenades[s].joint = Some(f);
        self.fixed_joint_set_connected_body(f, Some(super::BodyOwner::Physical(phys)));
        self.sticky_set_layer(s, DEBRIS_LAYER);
    }

    /// StickyGrenadeBehaviour.DestroyJoint
    pub(crate) fn sticky_destroy_joint(&mut self, s: usize) {
        self.sticky_set_layer(s, OBJECTS_LAYER);
        self.sticky_grenades[s].isStuck = false;
        if self.sticky_joint_alive(s) {
            self.destroy_joint_delayed(self.sticky_grenades[s].joint.unwrap());
        }
    }

    /// StickyGrenadeBehaviour.FixedUpdate
    pub(crate) fn sticky_grenade_fixed_update(&mut self, s: usize) {
        let alive = self.sticky_joint_alive(s);
        let stuck = self.sticky_grenades[s].isStuck;
        // !joint.connectedBody: its Rigidbody2D is gone (or never set).
        let no_connected_body = alive
            && match self.fixed_joints[self.sticky_grenades[s].joint.unwrap()].connected {
                None => true,
                Some(super::BodyOwner::Physical(p)) => self.destroyed_components.contains(&self.physicals[p].rigidbody.component),
                Some(super::BodyOwner::Node(k)) => self.destroyed_components.contains(&self.node_rigidbodies[k].rigidbody.component),
            };
        if (stuck && !alive) || (!stuck && alive) || (stuck && alive && no_connected_body) {
            self.sticky_destroy_joint(s);
        }
    }

    /// StickyGrenadeBehaviour.OnJointBreak2D, sent to the broken joint's GameObject: `broken != joint` compares the
    /// components.
    pub(crate) fn sticky_grenade_on_joint_break(&mut self, f: usize) {
        for s in 0..self.sticky_grenades.len() {
            if self.sticky_grenades[s].joint == Some(f) {
                self.sticky_destroy_joint(s);
            }
        }
    }

    /// Object.Destroy(FixedJoint2D): DestroyObjectDelayed queues it at min(fixedTimeAsDouble, the frame's time).
    pub(super) fn destroy_joint_delayed(&mut self, f: usize) {
        let at = self.time.fixedTimeAsDouble.min(self.time.timeAsDouble);
        self.joint_destroys.push((at, f));
        // DestroyObjectFromScripting (1808cd940) disables a Behaviour at once: the b2 joint goes before the next step.
        self.fixed_joints[f].enabled = false;
        self.fixed_joint_cleanup(f, false);
    }

    /// The queued joint Destroys due by now, as (time, `fixed_joints` index).
    pub fn joint_due_destroys(&self, now: f64) -> Vec<(f64, usize)> {
        self.joint_destroys.iter().copied().filter(|&(at, _)| at <= now).collect()
    }

    /// The delayed Destroy of a FixedJoint2D (a second one finds it gone).
    pub fn run_joint_destroy(&mut self, f: usize) {
        self.joint_destroys.retain(|&(_, j)| j != f);
        if !self.fixed_joints[f].destroyed {
            self.fixed_joint_destroy(f);
        }
    }

    /// StickyGrenadeBehaviour.IsStickyWith: the object's PhysicalProperties are not in unstickyPropertySet.
    pub(crate) fn sticky_is_sticky_with(&self, s: usize, p: usize) -> bool {
        !self.sticky_grenades[s].unsticky.contains(&self.physicals[p].Properties)
    }

    /// StickyGrenadeBehaviour.OnCollisionEnter2D's StickyGrenadeBehaviour.GetPhysicalBehaviour: Global.PhysicalObjectsInWorldByTransform[collision.transform]: the other body's transform, else the collider's.
    pub(crate) fn sticky_get_physical_behaviour(&self, c: &ScriptCollision) -> Option<usize> {
        match c.rigidbody {
            Some(rb) => self.physicals.iter().position(|p| !p.destroyed && p.in_world && p.rigidbody.component == rb),
            None => self.physical_of_collider(c.collider).filter(|&p| self.physicals[p].in_world),
        }
    }

    pub(crate) fn sticky_grenade_collision_enter(&mut self, s: usize, c: &ScriptCollision) {
        let g = &self.sticky_grenades[s];
        if g.isStuck || !self.explosives[g.explosive].Armed {
            return;
        }
        if let Some(p) = self.sticky_get_physical_behaviour(c).filter(|&p| self.sticky_is_sticky_with(s, p)) {
            self.sticky_grenades[s].isStuck = true;
            self.sticky_create_joint(s, p);
        }
    }
}

/// DisintegrationCounterBehaviour on a root that is not a person's: the root's top object (component 0 id) and the
/// PhysicalBehaviours (`Game::physicals`) Start subscribed OnChildDisintegrated to.
#[derive(Clone, Debug)]
pub struct DisintegrationCounter {
    pub id: i32,
    pub root: i32,
    pub DisintegrationCount: i32,
    pub children: Vec<usize>,
    /// Each OnDisintegration subscription it holds (a second Start subscribes `children` again; the old ones stay).
    pub subs: Vec<usize>,
    /// OnDestroy ran: the subscriptions it did not remove still call OnChildDisintegrated on the destroyed component.
    pub destroyed: bool,
}

impl Game {
    /// DisintegrationCounterBehaviour.Start: GetComponentsInChildren<PhysicalBehaviour>() (the active objects below,
    /// depth first, each object's components in order).
    pub fn disintegration_counter_start(&mut self, id: i32) {
        let (root, ids, own) = self.with_component_instance(id, |_, inst, o| {
            fn collect(inst: &super::spawn::Instance, x: usize, out: &mut Vec<i32>) {
                if !inst.objects[x].active {
                    return;
                }
                for (k, (_, c)) in inst.prefab.objects[x].components.iter().enumerate() {
                    if matches!(c, crate::assets::db::Component::Script { class, .. } if class == "PhysicalBehaviour") {
                        out.push(inst.objects[x].components[k]);
                    }
                }
                for &child in &inst.prefab.objects[x].children {
                    collect(inst, child, out);
                }
            }
            let mut ids = Vec::new();
            collect(inst, o, &mut ids);
            let mut top = o;
            while let Some(parent) = inst.objects[top].parent {
                top = parent;
            }
            (inst.objects[top].components[0], ids, inst.objects[o].components.clone())
        });
        let children: Vec<usize> = ids.iter().map(|&cid| self.physicals.iter().position(|p| p.id == cid).expect("a PhysicalBehaviour in the arena")).collect();
        // The Human's own counter (beside its PersonBehaviour): a later Start replaces children and subscribes each again.
        if let Some(person) = self.people.iter().position(|p| own.contains(&p.id)) {
            self.people[person].counter_subs.extend(&children);
            self.people[person].counter_children = Some(children);
            return;
        }
        if let Some(x) = self.disintegration_counters.iter_mut().find(|x| x.id == id) {
            x.subs.extend(&children);
            x.children = children;
            return;
        }
        self.disintegration_counters.push(DisintegrationCounter { id, root, DisintegrationCount: 0, subs: children.clone(), children, destroyed: false });
    }

    /// EggBehaviour.OnEnable: `gameObject.name = Number + " button"`; returns the name. Nothing reads it: the buttons are Abyss map
    /// objects (no PhysicalBehaviour, so never in SelectionController's root names or a save's ObjectState hierarchy paths), and the
    /// game's only name lookups (Transform.Find in DecalControllerBehaviour, ObjectState and Utils.GetOutlineSprite) look for
    /// other children; the rewrite keeps no GameObject names for it.
    pub fn egg_on_enable(&self, e: usize) -> String {
        format!("{} button", self.eggs[e].Number)
    }

    /// EggBehaviour.Update: a click on the key while the 14 digits are not all in types its Number. The click sound is
    /// audio.
    pub fn egg_update(&mut self, i: usize) {
        if !self.egg_can_enter {
            return;
        }
        // `!TextMesh || TextMesh.text.Length != 14`: a null TextMesh passes, then throws below.
        if let Some(t) = self.eggs[i].TextMesh {
            if self.text_get(t).encode_utf16().count() == 14 {
                return;
            }
        }
        if !self.input.down(crate::il!(str "EggBehaviour::Update" 0)) {
            return;
        }
        let e = self.eggs[i].clone();
        let (c, x) = self.with_component_instance(e.id, |_, inst, o| crate::unity::transform::transform_aabb(e.bounds.0, e.bounds.1, &inst.chain(o)));
        let m = self.MousePosition;
        if (0..3).all(|k| m[k] >= c[k] - x[k] && m[k] <= c[k] + x[k]) {
            let t = Self::text_ref(e.TextMesh);
            let typed = format!("{}{}", self.text_get(t), e.Number);
            self.text_set(t, &typed);
            if typed.encode_utf16().count() == 14 {
                self.egg_entered = typed;
                self.egg_can_enter = false;
                let id = e.id;
                self.start_coroutine(id, id, Self::egg_validate_routine(i));
            }
        }
    }

    /// EggBehaviour.Validate.
    pub(crate) fn egg_validate_routine(i: usize) -> Routine {
        let mut stage = 0;
        Box::new(move |g| {
            stage += 1;
            match stage {
                1 => Yield::WaitForSeconds(0.3),
                2 => {
                    let t = Self::text_ref(g.eggs[i].TextMesh);
                    g.text_color_set(t, [0.0, 1.0, 1.0, 1.0]);
                    g.text_set(t, "Checking~");
                    Yield::WaitForSeconds(0.3)
                }
                _ => {
                    // StartCoroutine(controller.CheckCode(enteredCode, callback)) on this egg; controller is
                    // GetComponentInParent<OffsetGridBehaviour>().
                    let id = g.eggs[i].id;
                    // GetComponentInParent<OffsetGridBehaviour>() (the egg's own object first); null throws at controller.CheckCode.
                    let grid_ids: Vec<i32> = g.offset_grids.iter().map(|x| x.id).collect();
                    let grid = g.with_component_instance(id, |_, inst, o| {
                        let mut x = Some(o);
                        while let Some(k) = x {
                            if let Some(&found) = grid_ids.iter().find(|gid| inst.objects[k].components.contains(gid)) {
                                return Some(found);
                            }
                            x = inst.objects[k].parent;
                        }
                        None
                    });
                    let Some(grid) = grid else { unimplemented!("mod-only: EggBehaviour.Validate without an OffsetGridBehaviour in its parents") };
                    g.start_coroutine(id, id, Self::offset_grid_check_code_routine(i, grid, g.egg_entered.clone()));
                    Yield::Done
                }
            }
        })
    }

    /// OffsetGridBehaviour.CheckCode (rewrite feature: the code is checked here, not by the developer's web server,
    /// so the error result never comes). The sounds are audio.
    pub(crate) fn offset_grid_check_code_routine(egg: usize, grid: i32, given: String) -> Routine {
        let mut stage = 0;
        let r = given == "62831853071795";
        Box::new(move |g| {
            stage += 1;
            match stage {
                // WaitUntil(() => task.IsCompleted): the check is already done.
                1 => Yield::WaitUntil(Box::new(|_| true)),
                2 => {
                    g.egg_check_code_callback(egg, if r { OffsetGridResult::V } else { OffsetGridResult::I });
                    if r { Yield::WaitForSeconds(0.5) } else { Yield::Done }
                }
                _ => {
                    g.start_coroutine(grid, grid, Self::offset_grid_inform_routine(egg));
                    Yield::Done
                }
            }
        })
    }

    /// OffsetGridBehaviour.Inform: the end file comes from the PPG_WEB_DIR folder (rewrite feature: nothing is
    /// downloaded) and is written as modulo2.jpg in the rewrite's save folder; a failure is the Debug.LogError catch.
    pub(crate) fn offset_grid_inform_routine(egg: usize) -> Routine {
        let mut stage = 0;
        Box::new(move |g| {
            stage += 1;
            if stage == 1 {
                let copied = std::env::var_os("PPG_WEB_DIR").zip(super::stats::save_dir()).ok_or(()).and_then(|(web, save)| {
                    let bytes = std::fs::read(std::path::Path::new(&web).join("modulo2.jpg")).map_err(|_| ())?;
                    std::fs::write(save.join("modulo2.jpg"), bytes).map_err(|_| ())
                });
                // catch (Exception message): Debug.LogError(message), console only.
                let _ = copied;
                return Yield::WaitUntil(Box::new(|_| true));
            }
            g.egg_check_code_callback(egg, OffsetGridResult::C);
            Yield::Done
        })
    }

    /// EggBehaviour.Validate's CheckCode callback; Flash runs on the egg.
    fn egg_check_code_callback(&mut self, egg: usize, result: OffsetGridResult) {
        let id = self.eggs[egg].id;
        let t = Self::text_ref(self.eggs[egg].TextMesh);
        match result {
            OffsetGridResult::I => {
                self.text_set(t, "Incorrect");
                self.text_color_set(t, [1.0, 0.92156863, 0.015686275, 1.0]);
                self.start_coroutine(id, id, Self::egg_flash_routine(egg, true, ""));
            }
            OffsetGridResult::V => {
                self.text_set(t, "Receiving...");
                self.text_color_set(t, [1.0, 1.0, 1.0, 1.0]);
                self.egg_can_enter = false;
            }
            OffsetGridResult::C => {
                self.text_set(t, "modulo2.jpg");
                self.text_color_set(t, [0.0, 1.0, 0.0, 1.0]);
                self.start_coroutine(id, id, Self::egg_flash_routine(egg, false, "modulo2.jpg"));
            }
        }
    }

    /// EggBehaviour.Flash: TextMesh.enabled = true, eight toggles 0.1 s apart, then the end text, enabled, white and canEnter.
    fn egg_flash_routine(egg: usize, again: bool, end: &'static str) -> Routine {
        let mut i = -1;
        Box::new(move |g| {
            let t = Self::text_ref(g.eggs[egg].TextMesh);
            if i == -1 {
                g.text_enabled_set(t, true);
                i = 0;
            }
            if i < 8 {
                i += 1;
                let on = !g.world_texts[t].enabled;
                g.text_enabled_set(t, on);
                return Yield::WaitForSeconds(0.1);
            }
            g.text_set(t, end);
            g.text_enabled_set(t, true);
            g.text_color_set(t, [1.0, 1.0, 1.0, 1.0]);
            g.egg_can_enter = again;
            Yield::Done
        })
    }

    /// OffsetGridBehaviour.Update: Blip's GameObject flips every 1.9 seconds.
    pub fn offset_grid_update(&mut self, i: usize) {
        // OffsetGridBehaviour.Awake: audioSource = GetComponent<AudioSource>(), audio only (CheckCode and Inform ask the developer's web server: not rewritten)
        self.offset_grids[i].timer += self.time.deltaTime;
        if self.offset_grids[i].timer > 1.9 {
            self.offset_grids[i].timer = 0.0;
            self.with_component_instance(self.offset_grids[i].Blip, |g, inst, o| {
                let on = inst.objects[o].self_active;
                g.set_active(inst, o, !on);
            });
        }
    }

    /// OnlyExistWith.Update: once Other is gone, Object.Destroy(AlsoDelete) and Object.Destroy(this). Both destroys land at
    /// the end of this frame, before the next physics step, so the joint goes now.
    pub fn only_exist_with_update(&mut self, i: usize) {
        let s = self.only_exist_withs[i].clone();
        let hinge = self.anchored_joints.iter().find(|a| a.component == s.Other).and_then(|a| a.hinge);
        if self.destroyed_components.contains(&s.Other) || hinge.is_some_and(|h| self.hinges[h].destroyed) {
            self.event_call(super::link::EventCall::DisableJoint(s.AlsoDelete));
            self.destroyed_components.insert(s.AlsoDelete);
            // gapB r15: Object.Destroy(this) as any script's: enabled = false now, gone in the delayed destroy.
            self.tr_destroy_component(s.id, 0.0);
        }
    }

    /// ActOnDisintegrate.OnDisintegration of the components subscribed to physical `p` (ActOnDisintegrate.Awake subscribes: the arena entry; ActOnDisintegrate.OnDestroy unsubscribes: destroyed_components).
    pub fn act_on_disintegrate_run(&mut self, p: usize) {
        for i in 0..self.act_on_disintegrates.len() {
            let a = &self.act_on_disintegrates[i];
            if a.physical == p {
                for _ in 0..self.listen_count(a.id, super::continuous::Ev::OnDisintegration) {
                    self.act_on_disintegrate_on_disintegration(i);
                }
            }
        }
    }

    /// ActOnDisintegrate.Awake: PhysicalBehaviour.OnDisintegration += OnDisintegration.
    pub fn act_on_disintegrate_awake(&mut self, i: usize) {
        self.listen_add(self.act_on_disintegrates[i].id, super::continuous::Ev::OnDisintegration);
    }

    /// ActOnDisintegrate.OnDestroy: PhysicalBehaviour.OnDisintegration -= OnDisintegration.
    pub fn act_on_disintegrate_on_destroy(&mut self, i: usize) {
        self.listen_remove(self.act_on_disintegrates[i].id, super::continuous::Ev::OnDisintegration);
    }

    /// ActOnDisintegrate.OnDisintegration (one subscribed component): its Event when enabled.
    pub(crate) fn act_on_disintegrate_on_disintegration(&mut self, i: usize) {
        let a = &self.act_on_disintegrates[i];
        if a.enabled {
            for call in a.calls.clone() {
                self.event_call(call);
            }
        }
    }

    /// The OnDisintegration of physical `p` reaches the counters subscribed to it.
    pub fn disintegration_counters_on_disintegration(&mut self, p: usize) {
        let mut i = 0;
        while i < self.disintegration_counters.len() {
            let id = self.disintegration_counters[i].id;
            let mut gone = false;
            for _ in 0..self.disintegration_counters[i].subs.iter().filter(|&&q| q == p).count() {
                self.disintegration_counter_on_child_disintegrated(i);
                // The handler may have destroyed the root (and this counter with it).
                gone = self.disintegration_counters.get(i).is_none_or(|x| x.id != id);
                if gone {
                    break;
                }
            }
            if !gone {
                i += 1;
            }
        }
    }

    /// DisintegrationCounterBehaviour.OnChildDisintegrated
    pub(crate) fn disintegration_counter_on_child_disintegrated(&mut self, i: usize) {
        self.disintegration_counters[i].DisintegrationCount += 1;
        let alive = self.disintegration_counters[i].children.iter().filter(|&&c| !self.physicals[c].destroyed).count() as i32;
        if self.disintegration_counters[i].DisintegrationCount >= alive {
            // base.transform on a destroyed component (a counter started twice keeps a subscription past its OnDestroy).
            if self.disintegration_counters[i].destroyed {
                super::menu::throw("MissingReferenceException: The object of type 'DisintegrationCounterBehaviour' has been destroyed but you are still trying to access it.
Your script should either check if it is null or you should not destroy the object.");
            }
            let root = self.disintegration_counters[i].root;
            self.destroy_object(root);
        }
    }

    /// DisintegrationCounterBehaviour.OnDestroy: `OnDisintegration -= OnChildDisintegrated` once per live child (a
    /// delegate removal takes one subscription); the rest stay on the destroyed component (a second Start's).
    pub fn disintegration_counter_destroy(&mut self, id: i32) {
        let Some(i) = self.disintegration_counters.iter().position(|c| c.id == id) else { return };
        let children = self.disintegration_counters[i].children.clone();
        for c in children.into_iter().filter(|&c| !self.physicals[c].destroyed) {
            let subs = &mut self.disintegration_counters[i].subs;
            if let Some(k) = subs.iter().rposition(|&q| q == c) {
                subs.remove(k);
            }
        }
        // Subscriptions left only on destroyed physicals can never fire again.
        if self.disintegration_counters[i].subs.iter().all(|&q| self.physicals[q].destroyed) {
            self.disintegration_counters.remove(i);
        } else {
            self.disintegration_counters[i].destroyed = true;
        }
    }
}

// Presentation-only methods (C# read: each writes only a material, property block, gizmo or sound, and nothing the game reads back):
// Vila.Awake: presentation (cam = Camera.main, read only by OnWillRenderObject)
// Vila.OnWillRenderObject: presentation (material.SetVector/SetFloat of the camera offset and ortho size on its own shader)
// OrganPulseBehaviour.Update: presentation (renderer.material _PulsatingStrength/_PulsatingSpeed from TotalLiquidAmount, a read-only sum)
// OldSyringeBehaviour.OnWillRenderObject: never called: the class is abstract and the export has no subclass (stab.rs Lodged), so no renderer carries one.
// ArrayCloner.OnDrawGizmosSelected: presentation (editor Gizmos.DrawWireCube, never drawn in a player)

#[cfg(test)]
mod tests {
    use crate::play::{new_game, step};

    /// EggBehaviour.Validate on Abyss's keypad (inactive since 1.28.41, so set active first): a wrong code flashes back to an
    /// empty keypad, the right one ends on modulo2.jpg.
    #[test]
    fn keypad_code_check() {
        let Some(game) = crate::game_assets::find_game(None).ok() else { return };
        let mut g = new_game(&game, "Abyss", None, None, 0).unwrap();
        step(&mut g, 1.0 / 60.0).unwrap();
        // 1.28.41: Abyss/pe, the keypad, is inactive in the scene: no Awake or Update runs on the keys, and canEnter keeps
        // its static initialiser's true (scene.rs load_main).
        let ids: Vec<i32> = g.eggs.iter().map(|e| e.id).collect();
        assert!(ids.len() == 10 && g.egg_can_enter);
        assert!(ids.iter().all(|&id| g.with_component_instance(id, |_, inst, o| !inst.objects[o].active && !inst.objects[o].activated)));
        // pe.SetActive(true): the keys' Awake and OnEnable.
        g.with_component_instance(ids[0], |g, inst, mut o| {
            while inst.objects[o].self_active {
                o = inst.objects[o].parent.expect("an inactive ancestor");
            }
            g.set_active_scripts(inst, o, true);
        });
        step(&mut g, 1.0 / 60.0).unwrap();
        assert!(g.egg_can_enter);
        for (code, text, again) in [("12345678901234", "", true), ("62831853071795", "modulo2.jpg", false)] {
            let t = g.eggs[0].TextMesh.unwrap();
            g.text_set(t, code);
            g.egg_entered = code.into();
            g.egg_can_enter = false;
            let id = g.eggs[0].id;
            g.start_coroutine(id, id, crate::game::Game::egg_validate_routine(0));
            for _ in 0..150 {
                step(&mut g, 1.0 / 60.0).unwrap();
            }
            assert_eq!((g.text_get(t), g.egg_can_enter), (text, again));
        }
    }
}

/// EggBehaviour.canEnter's initialiser (static).
pub const CAN_ENTER: bool = true;
/// EggBehaviour.enteredCode's initialiser (static).
pub const ENTERED_CODE: &str = "";

/// ActOnCollide.buffer: static ContactPoint2D[4] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const ACT_ON_COLLIDE_BUFFER: usize = 4;
