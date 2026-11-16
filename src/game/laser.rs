//! LaserBehaviour (the Laser Pointer and the Laser Attachment). Start and Use (when not an Attachment) run UpdateLaser:
//! the beam from the barrel through Utils.GetLaserEndPoint, which is detector.rs's get_laser_end_point. LateUpdate fires
//! while Activated: a hit physical gets heat, ignition, Slice and Break above charge thresholds, burn progress and, above
//! 700, an explosion every 0.12 s. OnLasered reaches only LaserReceiverBehaviour (below).
//! The beam colours, line, particles and the context menu are presentation. The hoverbox egg branch needs ConnectedEggs,
//! which only EnergyWireBehaviour raises (not rewritten, link refuses it), so it is refused in laser_late_update.
#![allow(non_snake_case)]

use super::detector::LaserHit;

/// Messages.ILasered.LaserArgs: the LaserBehaviour (`Game::lasers`) and its finalHit (LaserReceiverBehaviour.OnLasered reads neither).
#[derive(Default, Clone, Copy, Debug)]
#[allow(dead_code)]
pub(crate) struct LaserArgs {
    laser: usize,
    pub hit: LaserHit,
}

impl LaserArgs {
    /// LaserArgs..ctor(laser, hit)
    fn new(laser: usize, hit: LaserHit) -> Self {
        LaserArgs { laser, hit }
    }
}
use crate::assets::db::ObjRef;
use super::explosion::{EffectSize, ExplosionParameters};
use super::visual::{color_lerp, scale, Gradient};
use super::{d, mathf, script, Game};

script! {
    Laser {
        Range: f32 = 5000.0,
        UserSetColour: [f32; 4] = [0.0, 1.0, 0.0, 1.0],
        barrelPosition: [f32; 2] = [0.0, 0.0],
        barrelDirection: [f32; 2] = [0.0; 2],
        Activated: bool = false,
        Attachment: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// isFit: true until OnAfterDeserialise clears it.
        isFit: bool = true,
        /// PhysicalBehaviour (the object's own)
        physical: usize = 0,
        /// LayersToHit (LayerMask bits)
        LayersToHit: u32 = 0,
        /// The explosion timer that the 700+ charge path counts.
        timer: f32 = 0.0,
        /// ConnectedEggs: the energy wires joining it to a Hoverbox.
        ConnectedEggs: i32 = 0,
        /// hv: the PhysicalBehaviour of the HoverboxBehaviour ConnectEgg was given.
        hv: Option<usize> = None,
        /// pseudoCharge
        pseudoCharge: f32 = 0.0,
        /// finalHit as UpdateLaser left it: whether `hit` has a value, its rayDepth and its physicalBehaviour.
        final_hit: Option<(bool, usize, Option<usize>, [f32; 2])> = None,
        /// ep: the explosion egg prefab, and `e`, its clone's root Transform id.
        ep: Option<ObjRef> = None,
        e: Option<i32> = None,
        /// LineRenderer: its index in `Game::lines`.
        line: usize = 0,
        /// vertices: the line's points UpdateLaser last set (null before the first).
        vertices: Vec<[f32; 3]> = Vec::new(),
        /// LaserColourGradient
        LaserColourGradient: Option<Gradient> = None,
        /// The Transform ids of ImpactParticles' and SmokeParticles' objects (FireLaser moves them to the hit).
        particles: [Option<i32>; 2] = [None; 2],
    }
}

script! {
    LaserReceiver {
        DoubleTrigger: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// GetComponent<PhysicalBehaviour>() of its object (Start dereferences it).
        physical: usize = 0,
        /// The private laserTime and lastLaserTime.
        laserTime: f32 = 0.0,
        lastLaserTime: f32 = 0.0,
        /// Light (None when null, which Update and OnDisable throw on).
        Light: Option<super::visual::SpriteAt> = None,
    }
}

script! {
    LiquidLevelGate {
        Threshold: f32 = 0.5,
        TriggerOnExit: bool = false,
    }
    links {
        /// PhysicalBehaviour (a serialised reference on the same object).
        physical: usize = 0,
        enabled: bool = true,
        /// The component's instance id.
        id: i32 = 0,
        /// Cursor (a SpriteRenderer; null throws in Update) and CursorMaxDistance.
        Cursor: Option<super::visual::SpriteAt> = None,
        CursorMaxDistance: f32 = 0.0,
        /// PassthroughLight (a draw-only GameObject, `Game::set_active_plain`; null throws in OnWillRenderObject).
        PassthroughLight: Option<i32> = None,
    }
}

script! {
    KeyTrigger {
        DoubleTrigger: bool = false,
        SignalHeat: f32 = 0.0,
        SignalCooldownSpeed: f32 = 1.0,
    }
    links {
        /// KeyCode (the enum's value).
        KeyCode: i32 = 0,
        /// Description (only UpdateDisplay's text shows it).
        Description: String = String::new(),
        enabled: bool = true,
        /// PhysicalBehaviour (a serialised reference).
        physical: usize = 0,
        /// base.transform, and the SpriteRenderer on its GameObject whose OnWillRenderObject calls it (none: never).
        own: super::script_object::ObjectXf = Default::default(),
        drawn: Option<super::visual::SpriteAt> = None,
        /// The private shouldSendContinuous.
        shouldSendContinuous: bool = false,
        /// The component's instance id.
        id: i32 = 0,
        /// TextMesh and DescriptionGizmo (world_text.rs).
        TextMesh: Option<usize> = None,
        /// ActiveLight (OnEnable and OnDisable switch it).
        ActiveLight: Option<super::visual::SpriteAt> = None,
        DescriptionGizmo: Option<usize> = None,
        /// LightGradient (OnWillRenderObject colours ActiveLight with it).
        LightGradient: super::visual::Gradient = Default::default(),
    }
}

impl Game {
    // LiquidLevelGateBehaviour.Awake: OnContinuousUpdate.AddListener, left out of continuous.rs listeners because its callback does nothing.
    // LiquidLevelGateBehaviour.OnContinuousUpdate: sends only while passingThrough, which is never set (FixedUpdate throws), so it does nothing.
    // LiquidLevelGateBehaviour.OnDestroy: OnContinuousUpdate.RemoveListener, of a listener that was never kept.
    /// LiquidLevelGateBehaviour.Update: Cursor.transform.localPosition = (0, 0, CursorMaxDistance * Threshold).
    pub fn liquid_level_gate_update(&mut self, i: usize) {
        let s = &self.liquid_level_gates[i];
        let Some(cursor) = s.Cursor else { super::menu::npe() };
        let z = (d(s.CursorMaxDistance) * s.Threshold).f();
        self.set_sprite_local_position(cursor, [0.0, 0.0, z]);
    }

    /// LiquidLevelGateBehaviour.OnWillRenderObject for each gate whose sprite the camera draws: while enabled,
    /// PassthroughLight.SetActive(passingThrough), and passingThrough is always false.
    pub fn liquid_level_gates_will_render(&mut self) {
        for i in 0..self.liquid_level_gates.len() {
            let s = &self.liquid_level_gates[i];
            if !self.will_render_dispatch(s.id, s.enabled && !self.destroyed_components.contains(&s.id) && self.sprite_drawn(super::visual::SpriteAt::Physical(s.physical))) {
                continue;
            }
            match s.PassthroughLight {
                Some(light) => self.set_active_plain(light, false),
                None => self.unity_callback(|_| super::menu::npe()),
            }
        }
    }
    /// KeyTriggerBehaviour.Update. TriggerEditorBehaviour is never being edited.
    pub fn key_trigger_update(&mut self, k: usize) {
        self.key_triggers[k].shouldSendContinuous = false;
        if self.input.ui_block {
            return;
        }
        let c = self.input.key(self.key_triggers[k].KeyCode);
        let dt = self.time.deltaTime;
        if c.down {
            self.key_trigger_send_signal(k);
        }
        self.key_triggers[k].shouldSendContinuous = c.held;
        if self.key_triggers[k].DoubleTrigger && c.up {
            self.key_trigger_send_signal(k);
        }
        let x = &mut self.key_triggers[k];
        x.SignalHeat = mathf::clamp01((d(x.SignalHeat) - d(dt) * x.SignalCooldownSpeed).f());
    }

    /// KeyTriggerBehaviour.SendSignal: SendMessage("IsolatedActivation") on each channel to its own object.
    pub(crate) fn key_trigger_send_signal(&mut self, k: usize) {
        self.key_triggers[k].SignalHeat = 1.0;
        if self.key_triggers[k].enabled {
            self.send_isolated_xf(&self.key_triggers[k].own.clone(), false);
        }
    }

    // KeyTriggerBehaviour.Awake: the OnContinuousUpdate listener below; every key trigger is called from ContinuousActivationBehaviour.

    /// KeyTriggerBehaviour.UpdateDisplay
    pub fn key_trigger_update_display(&mut self, k: usize) {
        let x = &self.key_triggers[k];
        let name = super::menu::key_name(x.KeyCode);
        let gizmo_text = if x.KeyCode == 0 {
            String::new()
        } else if x.Description.chars().all(char::is_whitespace) {
            format!("Press {name}")
        } else {
            format!("Press {name} to {}", x.Description)
        };
        let (mesh, gizmo) = (x.TextMesh, x.DescriptionGizmo);
        self.text_write(mesh, &name);
        self.text_write(gizmo, &gizmo_text);
        let gizmo = Self::text_ref(gizmo);
        self.text_set_rotation_identity(gizmo);
    }

    /// KeyTriggerBehaviour.OnEnable: UpdateDisplay, TextMesh.enabled = true, then ActiveLight.enabled = true.
    pub fn key_trigger_on_enable(&mut self, k: usize) {
        self.key_trigger_update_display(k);
        let mesh = Self::text_ref(self.key_triggers[k].TextMesh);
        self.text_enabled_set(mesh, true);
        let light = self.key_triggers[k].ActiveLight.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.set_sprite_enabled(light, true);
    }

    /// KeyTriggerBehaviour.OnDisable: TextMesh.enabled = false, then ActiveLight.enabled = false.
    pub fn key_trigger_on_disable(&mut self, k: usize) {
        let mesh = Self::text_ref(self.key_triggers[k].TextMesh);
        self.text_enabled_set(mesh, false);
        let light = self.key_triggers[k].ActiveLight.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.set_sprite_enabled(light, false);
    }

    /// KeyTriggerBehaviour.OnWillRenderObject for each trigger whose sprite the camera draws: an active DescriptionGizmo gets localScale one.
    pub fn key_triggers_will_render(&mut self) {
        for k in 0..self.key_triggers.len() {
            let s = &self.key_triggers[k];
            if !self.will_render_dispatch(s.id, s.enabled && !self.destroyed_components.contains(&s.id) && s.drawn.is_some_and(|d| self.sprite_drawn(d))) {
                continue;
            }
            self.key_trigger_will_render(k);
        }
    }
    /// KeyTriggerBehaviour.OnWillRenderObject: ActiveLight.color = LightGradient.Evaluate(SignalHeat) (a null ActiveLight
    /// throws before the gizmo); then an active DescriptionGizmo's localScale = Vector3.one.
    pub fn key_trigger_will_render(&mut self, k: usize) {
        let Some(light) = self.key_triggers[k].ActiveLight else {
            super::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        };
        let c = self.key_triggers[k].LightGradient.evaluate(self.key_triggers[k].SignalHeat);
        self.set_sprite_color(light, c);
        let gizmo = Self::text_ref(self.key_triggers[k].DescriptionGizmo);
        if self.world_texts[gizmo].visible {
            self.text_set_local_scale_one(gizmo);
        }
    }
    /// KeyTriggerBehaviour.OnContinuousUpdate, a ContinuousActivationBehaviour listener from Awake that is never removed.
    pub(crate) fn key_trigger_continuous_update(&mut self, k: usize) {
        if !self.key_triggers[k].shouldSendContinuous {
            return;
        }
        if self.xf_destroyed(&self.key_triggers[k].own, self.key_triggers[k].id, "KeyTriggerBehaviour") {
            super::menu::throw("MissingReferenceException: The object of type 'KeyTriggerBehaviour' has been destroyed but you are still trying to access it.");
        }
        if self.key_triggers[k].enabled {
            let x = &mut self.key_triggers[k];
            x.SignalHeat = mathf::max(0.5, x.SignalHeat);
            self.send_isolated_xf(&self.key_triggers[k].own.clone(), true);
        }
    }

    /// LaserBehaviour.BarrelPosition: rigidbody.GetRelativePoint(barrelPosition * lossyScale).
    pub(crate) fn laser_barrel_position(&mut self, l: usize) -> [f32; 2] {
        let p = self.lasers[l].physical;
        let s = self.lossy_scale(p);
        let b = self.lasers[l].barrelPosition;
        let v = [b[0] * s[0], b[1] * s[1]];
        self.sync_pending();
        let xf = self.world.body(self.physicals[p].rigidbody.body).xf;
        [xf.q.c * v[0] - xf.q.s * v[1] + xf.p.x, xf.q.s * v[0] + xf.q.c * v[1] + xf.p.y]
    }

    /// LaserBehaviour.BarrelDirection: rigidbody.GetRelativeVector(barrelDirection), negated when lossyScale.x is negative.
    pub(crate) fn laser_barrel_direction(&mut self, l: usize) -> [f32; 2] {
        let p = self.lasers[l].physical;
        let b = self.lasers[l].barrelDirection;
        self.sync_pending();
        let xf = self.world.body(self.physicals[p].rigidbody.body).xf;
        let v = [xf.q.c * b[0] - xf.q.s * b[1], xf.q.s * b[0] + xf.q.c * b[1]];
        // LaserBehaviour.IsFlipped: transform.lossyScale.x < 0.
        let sign = if self.lossy_scale(p)[0] < 0.0 { -1.0 } else { 1.0 };
        [v[0] * sign, v[1] * sign]
    }

    /// LaserBehaviour.UpdateLaser: the beam's hits from the barrel. Its result is the last hit's physical when there is
    /// one (`finalHit`, as the caller's Some), and the colours, line and particles it sets are presentation.
    pub(crate) fn laser_update(&mut self, l: usize) -> Option<(usize, LaserHit)> {
        let origin = self.laser_barrel_position(l);
        let direction = self.laser_barrel_direction(l);
        let (layers, range) = (self.lasers[l].LayersToHit, self.lasers[l].Range);
        // The colour: UserSetColour with alpha 1 times 0.02 * Clamp(Charge, 1, 50), lerped to the gradient by Charge / 1000.
        let charge = self.physicals[self.lasers[l].physical].Charge();
        let num = (d(charge) / 1000.0f32).f();
        let mut user = self.lasers[l].UserSetColour;
        user[3] = 1.0;
        let user = scale(user, (d(0.02f32) * mathf::clamp(charge, 1.0, 50.0)).f());
        let color = if num > f32::from_bits(1) {
            color_lerp(user, self.lasers[l].LaserColourGradient.as_ref().expect("LaserBehaviour.LaserColourGradient").evaluate(num), num)
        } else {
            user
        };
        let line = self.lasers[l].line;
        self.lines[line].gradient.set_start_color(color);
        self.lines[line].gradient.set_end_color(color);
        let mut hits: Vec<LaserHit> = Vec::new();
        self.get_laser_end_point(origin, direction, &mut hits, layers, range, 64);
        if hits.is_empty() {
            self.line_set_position_count(line, 2);
            let points = vec![[origin[0], origin[1], 0.0], [(d(origin[0]) + (d(direction[0]) * range).f()).f(), (d(origin[1]) + (d(direction[1]) * range).f()).f(), 0.0]];
            self.line_set_positions(line, &points);
            self.lasers[l].vertices = points;
        } else {
            self.line_set_position_count(line, hits.len() + 1);
            let points: Vec<[f32; 3]> = std::iter::once([origin[0], origin[1], 0.0]).chain(hits.iter().map(|h| [h.point[0], h.point[1], 0.0])).collect();
            self.line_set_positions(line, &points);
            self.lasers[l].vertices = points;
        }
        let last = *hits.last()?;
        self.lasers[l].final_hit = Some((last.hit.is_some(), last.rayDepth as usize, last.physical, last.point));
        last.physical.map(|p| (p, last))
    }

    /// LaserBehaviour.UpdateActivation: UpdateLaser while Activated and enabled, ImpactParticles/SmokeParticles.Play(), then the
    /// line on; otherwise both Stop() and the line off.
    pub(crate) fn laser_update_activation(&mut self, l: usize) {
        if self.lasers[l].Activated && self.lasers[l].enabled {
            self.laser_update(l);
            self.laser_particles(l, super::PsCall::Play);
            let line = self.lasers[l].line;
            self.lines[line].enabled = true;
        } else {
            self.laser_particles(l, super::PsCall::Stop(self.time.timeAsDouble));
            let line = self.lasers[l].line;
            self.lines[line].enabled = false;
        }
    }

    /// ImpactParticles then SmokeParticles: `call` (Play or Stop).
    fn laser_particles(&mut self, l: usize, call: super::PsCall) {
        let id = self.lasers[l].id;
        self.ps_field_call(id, "ImpactParticles", true, call);
        self.ps_field_call(id, "SmokeParticles", true, call);
    }

    /// LaserBehaviour.Start: the context menu button (its condition, !ColorpickerDialogBehaviour.IsOpen, holds: no
    /// dialog is open while a menu is), the line's world space, then UpdateActivation.
    pub fn laser_start(&mut self, l: usize) {
        let line = self.lasers[l].line;
        self.lines[line].use_world_space = true;
        let p = self.lasers[l].physical;
        self.physicals[p].menu.push(super::menu::MenuButton::new("setLaserColour", crate::il!(str "LaserBehaviour::Start" 1), crate::il!(str "LaserBehaviour::Start" 2), super::menu::MenuAction::Entry(super::menu::Entry::LaserColour(l))));
        self.laser_update_activation(l);
    }

    /// LaserBehaviour.Use: `if (base.enabled && !Attachment)` Activated flips, then UpdateActivation.
    pub fn laser_use(&mut self, l: usize) {
        if self.lasers[l].enabled && !self.lasers[l].Attachment {
            self.lasers[l].Activated = !self.lasers[l].Activated;
            self.laser_update_activation(l);
        }
    }

    /// FireLaser's particle move, for a laser that hit a physical. The charge is Mathf.Max(Charge, pseudoCharge) with
    /// pseudoCharge 0 (only the refused egg branch sets it). Last, SendMessage("OnLasered") to the hit physical's object.
    /// Transform.position = `at` on a plain Transform below the laser's body: its local position under the live parent
    /// pose (the one ObjectState reads back), its local rotation kept.
    pub(crate) fn laser_particles_to(&mut self, tid: i32, at: [f32; 2]) {
        self.with_component_instance(tid, |g, inst, o| {
            let parent = inst.objects[o].parent.expect("a laser's particle system below the laser");
            let (t, q, s) = g.clone_pose(inst, parent);
            let chain = [crate::unity::transform::Trs { t: [t[0], t[1], t[2], 0.0], q, s: [s[0], s[1], s[2], 0.0] }];
            let ((local, _), _) = crate::unity::transform::set_world(&chain, [at[0], at[1], 0.0], q);
            inst.objects[o].trs.t = [local[0], local[1], local[2], inst.objects[o].trs.t[3]];
        });
    }

    /// LaserBehaviour.FireLaser
    pub(crate) fn laser_fire(&mut self, l: usize) {
        let Some((p, hit)) = self.laser_update(l) else { return };
        let dt = self.time.deltaTime;
        let num = mathf::max(self.physicals[self.lasers[l].physical].Charge(), self.lasers[l].pseudoCharge);
        if num > 2.0 {
            let cap = self.heat_capacity(p);
            self.physicals[p].Temperature += dt * num / cap;
            self.ignite_at(p, false, hit.point);
            // ImpactParticles.transform.position = SmokeParticles.transform.position = finalHit.point (z 0).
            for t in self.lasers[l].particles.into_iter().flatten() {
                self.laser_particles_to(t, hit.point);
            }
            if num > 50.0 {
                self.send_slice(p);
                let collider = hit.hit.expect("a laser hit on a physical has a RayHit2D").collider;
                self.send_break(collider, Some(p), [0.0, 0.0]);
            }
            if num > 150.0 {
                self.physicals[p].BurnProgress = 1.0;
                if num > 700.0 {
                    self.lasers[l].timer += dt;
                    if self.lasers[l].timer > 0.12 {
                        let under_water = self.is_point_under_water(hit.point);
                        let ex = ExplosionParameters {
                            FragmentationRayCount: 6,
                            Position: [hit.point[0], hit.point[1], 0.0],
                            Range: 1.0,
                            FragmentForce: 3.0,
                            CreateParticlesAndSound: true,
                            EffectSize: EffectSize::Medium,
                            DismemberChance: 0.0,
                            BallisticShrapnelCount: 0,
                        };
                        self.create_explosion_with_water(under_water, ex);
                        self.lasers[l].timer = 0.0;
                    }
                }
            }
        }
        if num > 0.1 {
            // Mono keeps the float sum in double until the store.
            self.physicals[p].BurnProgress = (d(self.physicals[p].BurnProgress) + d(dt) * num * 0.01f32).f();
        }
        // SendMessage("OnLasered", new LaserArgs(this, finalHit), DontRequireReceiver): LaserReceiverBehaviour is the only receiver.
        let args = LaserArgs::new(l, hit);
        for r in 0..self.laser_receivers.len() {
            if self.laser_receivers[r].physical == p {
                self.laser_receiver_on_lasered(r, args);
            }
        }
    }

    // LaserReceiverBehaviour.Awake: the OnContinuousUpdate listener, pushed at link time (`Listener::LaserReceiver`)
    // LaserReceiverBehaviour.OnDestroy: RemoveListener, `continuous.rs`'s `gone(id)`
    /// LaserReceiverBehaviour.OnDisable: Light.enabled = false.
    pub fn laser_receiver_on_disable(&mut self, r: usize) {
        self.laser_receiver_light(r, false);
    }

    fn laser_receiver_light(&mut self, r: usize, on: bool) {
        let light = self.laser_receivers[r].Light.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: LaserReceiverBehaviour.Light"));
        self.set_sprite_enabled(light, on);
    }

    /// LaserReceiverBehaviour.OnLasered: IsolatedActivation on each channel to its own object.
    fn laser_receiver_on_lasered(&mut self, r: usize, _args: LaserArgs) {
        if self.laser_receivers[r].laserTime < 0.1 && self.laser_receivers[r].enabled {
            self.send_isolated(self.laser_receivers[r].physical, false);
        }
        self.laser_receivers[r].laserTime = 0.2;
    }

    /// LaserReceiverBehaviour.OnContinuousUpdate
    pub(crate) fn laser_receiver_continuous_update(&mut self, r: usize) {
        if self.laser_receivers[r].laserTime > 0.1 && self.laser_receivers[r].enabled {
            self.send_isolated(self.laser_receivers[r].physical, true);
        }
    }

    /// LaserReceiverBehaviour.Update
    pub fn laser_receiver_update(&mut self, r: usize) {
        if self.paused || self.paused_menu {
            return;
        }
        let dt = self.time.deltaTime;
        let x = &self.laser_receivers[r];
        if x.DoubleTrigger && x.laserTime < 0.1 && x.lastLaserTime > 0.1 {
            self.send_isolated(x.physical, false);
        }
        let x = &mut self.laser_receivers[r];
        x.lastLaserTime = x.laserTime;
        x.laserTime -= dt;
        x.laserTime = mathf::clamp01(x.laserTime);
        let on = x.laserTime > 0.1;
        self.laser_receiver_light(r, on);
    }

    /// LaserBehaviour.OnAfterDeserialise: isFit = false, so LateUpdate stops after FireLaser.
    pub(crate) fn laser_on_after_deserialise(&mut self, l: usize) {
        self.lasers[l].isFit = false;
    }

    /// LaserBehaviour.LateUpdate: FireLaser while Activated, then nothing more once isFit is cleared by OnAfterDeserialise. The egg branch: a Hoverbox wired to the laser, moving above 5, with the beam's fourth segment on
    /// something soft, makes pseudoCharge 30 (and pushes the target by the hoverbox's velocity); its explosion-egg
    /// (`Instantiate(ep)`, a prefab with its own countdown) is not rewritten. Otherwise pseudoCharge 0, and a laser whose
    /// hoverbox is gone forgets its eggs.
    pub fn laser_late_update(&mut self, l: usize) {
        if self.lasers[l].Activated {
            self.laser_fire(l);
        }
        if !self.lasers[l].isFit {
            return;
        }
        let hv = self.lasers[l].hv.filter(|&h| self.wire_physical_alive(h));
        let point = self.lasers[l].final_hit.map_or([0.0; 2], |h| h.3);
        let hit = self.lasers[l].final_hit.and_then(|(has, depth, p, _)| (has && depth == 3).then_some(p).flatten()).filter(|&p| self.wire_physical_alive(p));
        // float.Epsilon is the smallest denormal.
        let soft = |g: &Self, p: usize| g.properties[g.physicals[p].Properties].Softness > f32::from_bits(1);
        let fast = |g: &Self, h: usize| mathf::magnitude2(g.physicals[h].rigidbody.velocity(&g.world)) > 5.0;
        if let (true, true, Some(h), Some(p)) = (self.lasers[l].Activated, self.lasers[l].ConnectedEggs > 0, hv, hit) {
            if soft(self, p) && fast(self, h) {
                let alive = self.lasers[l].e.filter(|e| !self.destroyed_components.contains(e));
                match alive {
                    None => {
                        let ep = self.lasers[l].ep.unwrap_or_else(|| super::menu::throw("ArgumentException: The Object you want to instantiate is null."));
                        self.lasers[l].e = Some(self.spawn_prefab(ep, [point[0], point[1], 0.0]));
                    }
                    Some(e) => self.with_component_instance(e, |_, inst, o| inst.set_world_position(o, [point[0], point[1], 0.0])),
                }
                self.lasers[l].pseudoCharge = 30.0;
                let velocity = self.physicals[h].rigidbody.velocity(&self.world);
                let rb = self.physicals[p].rigidbody.clone();
                rb.add_force_at_position(&mut self.world, velocity, point);
                self.laser_late_update_tail(l, hv);
                return;
            }
        }
        self.lasers[l].pseudoCharge = 0.0;
        self.laser_destroy_egg(l);
        self.laser_late_update_tail(l, hv);
    }

    /// The end of LaserBehaviour.LateUpdate: a laser whose hoverbox is gone forgets its eggs and its effect.
    fn laser_late_update_tail(&mut self, l: usize, hv: Option<usize>) {
        if self.lasers[l].ConnectedEggs > 0 && hv.is_none() {
            self.laser_destroy_egg(l);
            self.lasers[l].ConnectedEggs = 0;
        }
    }

    /// `if ((bool)e) Object.Destroy(e)`
    fn laser_destroy_egg(&mut self, l: usize) {
        if let Some(e) = self.lasers[l].e.filter(|e| !self.destroyed_components.contains(e)) {
            self.destroy_object(e);
        }
    }

    /// LaserBehaviour.OnDisable: Impact/SmokeParticles.Stop(), the line, then Object.Destroy(e).
    pub(crate) fn laser_on_disable(&mut self, l: usize) {
        self.laser_particles(l, super::PsCall::Stop(self.time.timeAsDouble));
        let line = self.lasers[l].line;
        self.lines[line].enabled = false;
        self.laser_destroy_egg(l);
    }

    /// LaserBehaviour.OnDestroy: Object.Destroy(e).
    pub(crate) fn laser_on_destroy(&mut self, l: usize) {
        self.laser_destroy_egg(l);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::menu::{Label, MenuAction};

    #[test]
    fn trigger_buttons_flip_and_relabel() {
        let mut g = Game::default();
        g.liquid_level_gates.push(LiquidLevelGate::default());
        g.key_triggers.push(KeyTrigger::default());
        assert_eq!(g.menu_label(Label::LiquidLevelGateTrigger(0)), crate::il!(str "ButtonBehaviour::<Start>b__9_0" 0));
        assert_eq!(g.menu_label(Label::KeyTriggerDouble(0)), crate::il!(str "ButtonBehaviour::<Start>b__9_0" 0));
        g.liquid_level_gates[0].TriggerOnExit = true;
        g.key_triggers[0].DoubleTrigger = true;
        assert_eq!(g.menu_label(Label::LiquidLevelGateTrigger(0)), crate::il!(str "ButtonBehaviour::<Start>b__9_0" 1));
        assert_eq!(g.menu_label(Label::KeyTriggerDouble(0)), crate::il!(str "ButtonBehaviour::<Start>b__9_0" 1));
        let _ = MenuAction::KeyTriggerDouble(0);
    }
}
