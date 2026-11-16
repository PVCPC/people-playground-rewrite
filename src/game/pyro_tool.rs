//! PyroPowerTool and its PyroPowerToolEntity. The tool's Awake instantiates the entity at the mouse; it follows the
//! mouse while the tool is current. While "drag" is held each fixed step sets fire to what is under the mouse, and a
//! fast mouse sweep blows a cone of RayCount rays along it that push, ignite and burn what they hit. Its three
//! ParticleSystems are embedded systems (`EmbeddedDrive::Pyro`): ParticleSystem by StartHold/StopHold's Play/Stop,
//! DirectionalPuff and LargePuff by SetFire's Emit(EmitParams) calls (`ps_calls`). The debug lines draw nothing.
#![allow(non_snake_case)]

use super::translated::rt::atan2;
use super::{mathf, script, Game};
use crate::b2::math::Vec2 as V;

script! {
    PyroEntity {
        Radius: f32 = 1.0,
        RayCount: i32 = 32,
        RayAngle: f32 = 25.0,
        MinRange: f32 = 1.0,
        Range: f32 = 20.0,
        Force: f32 = 1.0,
        IsCurrentTool: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Layers.m_Bits
        Layers: u32 = 0,
        isHeld: bool = false,
        mouseMovement: [f32; 2] = [0.0; 2],
        oldMousePos: [f32; 2] = [0.0; 2],
        blastSoundAvailable: bool = true,
    }
}

// PyroPowerTool.OnSelect: empty.
// PyroPowerTool.OnHold: empty.
// PyroPowerTool.OnFixedHold: empty.
// PyroPowerTool.OnDeselect: empty.
/// The PyroPowerTool component on the ToolController: pyroEntity, the `Game::pyro_entities` index.
pub struct PyroTool {
    pub entity: usize,
}

/// `EmbeddedDrive::Pyro` system: the ParticleSystem, LargePuff, DirectionalPuff fields.
pub const PYRO_SYSTEMS: [&str; 3] = ["ParticleSystem", "LargePuff", "DirectionalPuff"];

/// (float)Math.PI / 180f
const DEG2RAD: f32 = std::f32::consts::PI / 180.0;

impl Game {
    /// The PyroPowerTool is the current tool.
    pub(super) fn pyro_chosen(&self) -> bool {
        self.pyro_tool.as_ref().is_some_and(|t| self.pyro_entities[t.entity].IsCurrentTool)
    }

    /// PyroPowerTool.Awake, PyroPowerTool.OnToolChosen
    /// SetTool: GetComponent<PyroPowerTool>(), or AddComponent, whose Awake instantiates the entity at the mouse; then
    /// OnToolChosen.
    pub(crate) fn pyro_tool_choose(&mut self) {
        if self.pyro_tool.is_none() {
            self.allocate_instance_id();
            let db = self.assets.take().expect("Resources.Load without the game's data");
            let prefab = db.resource("Prefabs/PyroPowerEntity", 1).ok().flatten().expect("Resources.Load(Prefabs/PyroPowerEntity) found no GameObject");
            self.assets = Some(db);
            let n = self.pyro_entities.len();
            let mouse = self.MousePosition;
            self.spawn_prefab(prefab, mouse);
            assert_eq!(self.pyro_entities.len(), n + 1, "PyroPowerEntity without its PyroPowerToolEntity");
            self.pyro_tool = Some(PyroTool { entity: n });
            self.behaviours.add(0, super::Behaviour::PyroTool);
        }
        let e = self.pyro_tool.as_ref().unwrap().entity;
        self.pyro_entities[e].IsCurrentTool = true;
    }

    /// PyroPowerTool.OnToolUnchosen
    pub(crate) fn pyro_tool_unchoose(&mut self) {
        let e = self.pyro_tool.as_ref().unwrap().entity;
        self.pyro_stop_hold(e);
        self.pyro_entities[e].IsCurrentTool = false;
    }

    /// AudioSource or AudioSourceBlast (a null one throws).
    fn pyro_source(&self, e: usize, name: &str) -> i32 {
        self.audio.source_field(self.pyro_entities[e].id, name).unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
    }

    /// PyroPowerToolEntity.StopHold
    pub(crate) fn pyro_stop_hold(&mut self, e: usize) {
        self.ps_call(|x| matches!(x.drive, super::EmbeddedDrive::Pyro { e: pe, k: 0 } if pe == e), super::PsCall::Stop(self.time.timeAsDouble));
        let src = self.pyro_source(e, "AudioSource");
        self.audio.stop(src);
        if self.pyro_entities[e].isHeld {
            let clip = self.audio.clip(self.pyro_entities[e].id, "EndSound");
            self.audio.play_one_shot(src, clip.as_ref(), 1.0);
        }
        self.pyro_entities[e].isHeld = false;
    }

    /// The world pose of one of the entity's ParticleSystems (`chain`: its object up to the root): the root's TRS is
    /// the clone's live one, which Update moves to the mouse.
    pub(crate) fn pyro_ps_world(&self, e: usize, chain: &[crate::unity::transform::Trs]) -> ([f32; 3], [f32; 4], [f32; 3]) {
        let id = self.pyro_entities[e].id;
        let inst = self.unsynced.iter().chain(&self.retained).find(|i| i.objects.iter().any(|x| x.components.contains(&id))).expect("the PyroPowerEntity clone");
        let o = inst.objects.iter().position(|x| x.components.contains(&id)).unwrap();
        let mut chain = chain.to_vec();
        *chain.last_mut().expect("a ParticleSystem object") = inst.objects[o].trs;
        self.anchored_world(super::BareAnchor::Root, &chain)
    }

    /// PyroPowerTool.Update
    pub fn pyro_tool_update(&mut self) {
        let e = self.pyro_tool.as_ref().unwrap().entity;
        if !self.pyro_entities[e].IsCurrentTool {
            return;
        }
        if self.input.up(crate::il!(str "PyroPowerTool::Update" 0)) {
            self.pyro_stop_hold(e);
        }
        if !self.input.ui_block && !self.ui_lock() && !self.trigger_being_edited {
            let (id, mouse) = (self.pyro_entities[e].id, self.MousePosition);
            self.with_component_instance(id, |_, inst, o| inst.set_local_position(o, mouse));
            if self.input.down(crate::il!(str "PyroPowerTool::Update" 0)) {
                // PyroPowerToolEntity.StartHold
                self.pyro_entities[e].isHeld = true;
                self.ps_call(|x| matches!(x.drive, super::EmbeddedDrive::Pyro { e: pe, k: 0 } if pe == e), super::PsCall::Play);
                let src = self.pyro_source(e, "AudioSource");
                self.audio.play(src);
                let clip = self.audio.clip(id, "StartSound");
                self.audio.play_one_shot(src, clip.as_ref(), 1.0);
            }
        }
    }

    /// PyroPowerToolEntity.FixedUpdate
    pub fn pyro_entity_fixed_update(&mut self, e: usize) {
        if !self.pyro_entities[e].IsCurrentTool {
            return;
        }
        let m = self.MousePosition;
        let x = &mut self.pyro_entities[e];
        x.mouseMovement = [1.5 * (m[0] - x.oldMousePos[0]), 1.5 * (m[1] - x.oldMousePos[1])];
        if x.isHeld {
            self.pyro_set_fire(e);
        }
        self.pyro_entities[e].oldMousePos = [m[0], m[1]];
    }

    /// PyroPowerToolEntity.GetRayDir, PyroPowerToolEntity.GetRandomPosInCone: the angle of
    /// GetRayDir / GetRandomPosInCone's angle: Atan2(initial) + (Lerp(0, RayAngle, t) - RayAngle / 2) * Deg2Rad, in
    /// double and rounded at its local.
    pub(crate) fn pyro_angle(initial: [f32; 2], ray_angle: f32, t: f32) -> f32 {
        let l = mathf::lerp(0.0, ray_angle, t) as f64;
        (atan2(initial[1], initial[0]) as f64 + (l - ray_angle as f64 / 2.0) * DEG2RAD as f64) as f32
    }

    /// PyroPowerToolEntity.GetRayDir(initial, index): the angle at index / (RayCount - 1) of the cone (float division).
    pub(crate) fn pyro_ray_dir(initial: [f32; 2], ray_angle: f32, rays: i32, k: i32) -> [f32; 2] {
        let t = ((k as f32) as f64 / ((rays as f32) as f64 - 1.0)) as f32;
        let f = Self::pyro_angle(initial, ray_angle, t);
        [mathf::cos(f), mathf::sin(f)]
    }

    /// PyroPowerToolEntity.GetRandomPosInCone(initial, maxRange): the angle's Random.value, then the distance's;
    /// (dir * value) * maxRange.
    pub(crate) fn pyro_random_pos_in_cone(&mut self, ray_angle: f32, initial: [f32; 2], max_range: f32) -> [f32; 2] {
        let f = Self::pyro_angle(initial, ray_angle, self.random.value());
        let r = self.random.value();
        [mathf::cos(f) * r * max_range, mathf::sin(f) * r * max_range]
    }

    /// PyroPowerToolEntity.SetFire
    pub(crate) fn pyro_set_fire(&mut self, e: usize) {
        let id = self.pyro_entities[e].id;
        let position = self.with_component_instance(id, |_, inst, o| inst.position(o));
        let vector = V::new(position[0], position[1]);
        let num = self.time.deltaTime * 0.1;
        let x = &self.pyro_entities[e];
        let (radius, layers, range, min_range, rays, ray_angle, force) = (x.Radius, x.Layers, x.Range, x.MinRange, x.RayCount, x.RayAngle, x.Force);
        let mut hits = self.overlap_circle(V::new(self.MousePosition[0], self.MousePosition[1]), radius, layers);
        hits.truncate(PYRO_BUFFER);
        for (_, collider) in hits {
            // PhysicalObjectsInWorldByTransform.TryGetValue(collider2D.transform)
            if let Some(p) = self.physical_of_collider(collider).filter(|&p| self.collider_on_physical_object(collider, p) && self.physicals[p].in_world) {
                self.ignite(p, true);
                self.physicals[p].BurnProgress += num;
                let ph = &mut self.physicals[p];
                if ph.SimulateTemperature && ph.Temperature < 400.0 {
                    ph.Temperature += 40.0;
                }
            }
        }
        let movement = self.pyro_entities[e].mouseMovement;
        let num3 = mathf::min(mathf::sqr_magnitude(movement), range);
        if num3 < min_range {
            self.pyro_entities[e].blastSoundAvailable = true;
            return;
        }
        let normalized = mathf::normalized2(movement);
        if self.pyro_entities[e].blastSoundAvailable {
            self.pyro_entities[e].blastSoundAvailable = false;
            self.camera_shake_at(5.0, [position[0], position[1]], 0.8);
            // AudioSourceBlast.PlayOneShot(AudioSourceBlast.clip)
            let blast = self.pyro_source(e, "AudioSourceBlast");
            let clip = self.audio.get(blast).clip.clone();
            self.audio.play_one_shot(blast, clip.as_ref(), 1.0);
        }
        // Mathf.Atan2(0f - normalized.y, normalized.x) * 57.29578f + 90f, in double.
        let rotation = (atan2(-normalized[1], normalized[0]) as f64 * mathf::RAD2DEG as f64 + 90.0) as f32;
        for _ in 0..50 {
            let cone = self.pyro_random_pos_in_cone(ray_angle, normalized, num3);
            // sqrMagnitude / (num3 * num3), in double.
            let t = (mathf::sqr_magnitude(cone) as f64 / (num3 as f64 * num3 as f64)) as f32;
            let start_color = super::visual::color_lerp([0.5, 0.5, 0.5, 1.0], [0.0; 4], t);
            let position = Some(bevy::math::Vec3::new(vector.x + cone[0], vector.y + cone[1], 0.0));
            self.ps_emit_params(|x| matches!(x.drive, super::EmbeddedDrive::Pyro { e: pe, k: 2 } if pe == e), crate::particles::EmitParams { position, rotation: Some(rotation), start_color: Some(start_color), ..Default::default() });
        }
        for k in 0..rays {
            let dir = Self::pyro_ray_dir(normalized, ray_angle, rays, k);
            let Some(hit) = self.raycast(vector, dir, num3, layers, f32::NEG_INFINITY, f32::INFINITY) else { continue };
            let Some(p) = self.hit_world_physical(&hit) else { continue };
            let position = Some(bevy::math::Vec3::new(hit.point.x, hit.point.y, 0.0));
            self.ps_emit_params(|x| matches!(x.drive, super::EmbeddedDrive::Pyro { e: pe, k: 1 } if pe == e), crate::particles::EmitParams { position, ..Default::default() });
            let rb = &self.physicals[p].rigidbody;
            let k = ((force as f64) * mathf::sqrt(rb.mass(&self.world)) as f64 * num3 as f64) as f32;
            rb.add_force(&mut self.world, [k * dir[0], k * dir[1]]);
            self.ignite(p, true);
            let ph = &mut self.physicals[p];
            ph.BurnProgress += 0.2;
            ph.burnIntensity = 1.0;
        }
    }
}

/// PyroPowerToolEntity.buffer: static Collider2D[8] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const PYRO_BUFFER: usize = 8;
