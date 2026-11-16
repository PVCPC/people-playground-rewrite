//! LightningTool and its LightningToolEntityBehaviour. The tool's Awake instantiates the entity (inactive); choosing
//! the tool activates it. While "drag" is held the entity charges what is under the mouse, and a fast mouse flick
//! casts a bolt along the flick: an explosion at the hit, 150 charge to the object hit, then 2 to 4 flashes, each
//! showing the SingleBolt line (its Perlin points from the draws FireBolt makes) and the LargeFlashLight sprite.
#![allow(non_snake_case)]

use super::destroyable::Step;
use super::explosion::{EffectSize, ExplosionParameters, DEFAULT_RAYCAST_LAYERS};
use super::{d, mathf, script, Game};
use crate::b2::math::Vec2 as V;

/// LayerMask.GetMask("Objects", "Bounds")
const MASK: u32 = (1 << 9) | (1 << 11);

script! {
    LightningEntity {
        BoltThreshold: f32 = 2.0,
        CooldownTime: f32 = 0.5,
        Radius: f32 = 2.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Thunder.Length
        thunder: usize = 0,
        /// vertices.Length: SingleBolt.positionCount.
        vertices: usize = 0,
        delta: [f32; 3] = [0.0; 3],
        oldPos: [f32; 3] = [0.0; 3],
        cooldown: f32 = 0.0,
        isHeld: bool = false,
        /// SingleBolt: its index in `Game::lines`.
        bolt: Option<usize> = None,
        /// LargeFlashLight
        flash: Option<super::visual::SpriteAt> = None,
    }
}

/// FireBolt's line (LightningToolEntityBehaviour and WeatherLightningBehaviour): vertex i of n, at t = i / n, is
/// Lerp(from, to, t) + num * (GetPerlin2Mapped(x, t / num * 19) + insideUnitCircle * 0.2) * (1 - |2t - 1|), one
/// circle per vertex (`circles`, the draws FireBolt made).
pub(crate) fn lightning_bolt_points(from: [f32; 3], to: [f32; 2], num: f32, x: f32, circles: &[[f32; 2]]) -> Vec<[f32; 3]> {
    let n = circles.len();
    let to = [to[0], to[1], 0.0];
    circles
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let t = i as f32 / n as f32;
            let num4 = 1.0 - (2.0 * t - 1.0).abs();
            let p = mathf::perlin2_mapped(x, t / num * 19.0);
            let v = [(p[0] + c[0] * 0.2) * num4, (p[1] + c[1] * 0.2) * num4, 0.0];
            std::array::from_fn(|k| mathf::lerp(from[k], to[k], t) + num * v[k])
        })
        .collect()
}

// LightningTool.OnSelect: empty.
// LightningTool.OnHold: empty.
// LightningTool.OnFixedHold: empty.
// LightningTool.OnDeselect: empty.
/// The LightningTool component on the ToolController.
pub struct LightningTool {
    /// lightningEntity: the `Game::lightning_entities` index.
    pub entity: usize,
    /// It is the current tool.
    pub chosen: bool,
}

impl Game {
    /// SingleBolt.enabled and LargeFlashLight.enabled.
    pub(crate) fn lightning_entity_show(&mut self, e: usize, on: bool) {
        if let Some(l) = self.lightning_entities[e].bolt {
            self.lines[l].enabled = on;
        }
        if let Some(s) = self.lightning_entities[e].flash {
            self.set_sprite_enabled(s, on);
        }
    }

    /// The entity's root object id (its first component's instance id).
    fn lightning_root(&self, e: usize) -> i32 {
        self.lightning_entities[e].id
    }

    pub(crate) fn lightning_entity_active(&mut self, e: usize) -> bool {
        let id = self.lightning_root(e);
        self.with_component_instance(id, |_, inst, o| inst.objects[o].active)
    }

    fn lightning_entity_set_active(&mut self, e: usize, on: bool) {
        let id = self.lightning_root(e);
        self.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, on));
    }

    /// LightningTool.OnToolChosen
    /// SetTool: GetComponent<LightningTool>(), or AddComponent, whose Awake (LightningTool.Awake, lightning_tool_awake) instantiates the entity at the mouse and
    /// deactivates it; then OnToolChosen activates it.
    pub(crate) fn lightning_tool_choose(&mut self) {
        if self.lightning_tool.is_none() {
            self.allocate_instance_id();
            self.lightning_tool_awake();
        }
        let t = self.lightning_tool.as_mut().unwrap();
        t.chosen = true;
        let e = t.entity;
        self.lightning_entity_set_active(e, true);
    }

    /// LightningTool.Awake: a new LightningToolEntity at the mouse, deactivated, becomes lightningEntity (on a tool that has one
    /// already, as a direct call makes, the old entity stays in the scene).
    pub(crate) fn lightning_tool_awake(&mut self) {
        {
            let db = self.assets.take().expect("Resources.Load without the game's data");
            let prefab = db.resource("Prefabs/LightningToolEntity", 1).ok().flatten().expect("Resources.Load(Prefabs/LightningToolEntity) found no GameObject");
            self.assets = Some(db);
            let n = self.lightning_entities.len();
            let mouse = self.MousePosition;
            self.spawn_prefab(prefab, mouse);
            assert_eq!(self.lightning_entities.len(), n + 1, "LightningToolEntity without its LightningToolEntityBehaviour");
            self.lightning_entity_set_active(n, false);
            match self.lightning_tool.as_mut() {
                Some(t) => t.entity = n,
                None => {
                    self.lightning_tool = Some(LightningTool { entity: n, chosen: false });
                    self.behaviours.add(0, super::Behaviour::LightningTool);
                }
            }
        }
    }

    /// A loaded snapshot's LightningTool (the recorder's `LightningToolState active=`): made as SetTool makes it, its
    /// entity left as recorded (inactive while another tool is current, so its Update returns at once).
    pub(crate) fn lightning_tool_load(&mut self, active: bool, chosen: bool) {
        self.lightning_tool_choose();
        let t = self.lightning_tool.as_mut().unwrap();
        t.chosen = chosen;
        let e = t.entity;
        self.lightning_entity_set_active(e, active);
    }

    /// LightningTool.OnToolUnchosen
    pub(crate) fn lightning_tool_unchoose(&mut self) {
        let t = self.lightning_tool.as_mut().unwrap();
        t.chosen = false;
        let e = t.entity;
        self.lightning_entity_set_active(e, false);
    }

    /// LightningTool.Update
    pub fn lightning_tool_update(&mut self) {
        let e = self.lightning_tool.as_ref().unwrap().entity;
        if !self.lightning_entity_active(e) {
            return;
        }
        if self.input.up(crate::il!(str "LightningTool::Update" 0)) {
            self.lightning_stop_hold(e);
        }
        // No trigger editor is ever open.
        if !self.input.ui_block && !self.ui_lock() {
            let (id, mouse) = (self.lightning_root(e), self.MousePosition);
            self.with_component_instance(id, |_, inst, o| inst.set_local_position(o, mouse));
            if self.input.down(crate::il!(str "LightningTool::Update" 0)) {
                self.lightning_start_hold(e);
            }
        }
    }

    /// Input.mousePosition
    fn screen_mouse(&self) -> [f32; 3] {
        let s = self.input.screen();
        [s.mouse[0], s.mouse[1], 0.0]
    }

    /// LightningToolEntityBehaviour.StartHold (the component is always enabled).
    pub(crate) fn lightning_start_hold(&mut self, e: usize) {
        let m = self.screen_mouse();
        let id = self.lightning_entities[e].id;
        self.lightning_entity_show(e, false);
        // AudioSource.PlayOneShot(Ignite); LoopingAudio.Play(); ParticleSystem.Play()
        let (src, lp) = (self.lightning_source(id, "AudioSource"), self.lightning_source(id, "LoopingAudio"));
        let ignite = self.audio.clip(id, "Ignite");
        self.audio.play_one_shot(src, ignite.as_ref(), 1.0);
        self.audio.play(lp);
        self.ps_field_play_stop(id, "ParticleSystem", true);
        let x = &mut self.lightning_entities[e];
        x.isHeld = true;
        x.oldPos = m;
    }

    /// A serialised AudioSource field (a null one throws).
    fn lightning_source(&self, id: i32, field: &str) -> i32 {
        self.audio.source_field(id, field).unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
    }

    /// LightningToolEntityBehaviour.StopHold
    pub(crate) fn lightning_stop_hold(&mut self, e: usize) {
        let id = self.lightning_entities[e].id;
        let (src, lp) = (self.lightning_source(id, "AudioSource"), self.lightning_source(id, "LoopingAudio"));
        self.lightning_entity_show(e, false);
        self.audio.stop(lp);
        if self.lightning_entities[e].isHeld {
            let release = self.audio.clip(id, "Release");
            self.audio.play_one_shot(src, release.as_ref(), 1.0);
        }
        self.ps_field_play_stop(id, "ParticleSystem", false);
        self.lightning_entities[e].isHeld = false;
    }

    /// LightningToolEntityBehaviour.Update
    pub fn lightning_entity_update(&mut self, e: usize) {
        if !self.lightning_entities[e].isHeld {
            return;
        }
        let m = self.screen_mouse();
        let k = self.time.unscaledDeltaTime * 19.0;
        let x = &mut self.lightning_entities[e];
        x.delta = std::array::from_fn(|i| (m[i] - x.oldPos[i]) * k);
        x.oldPos = m;
        let radius = x.Radius;
        let mut hits = self.overlap_circle(V::new(self.MousePosition[0], self.MousePosition[1]), radius, DEFAULT_RAYCAST_LAYERS);
        hits.truncate(64);
        let dt = self.time.deltaTime;
        for (_, collider) in hits {
            // PhysicalObjectsInWorldByTransform.TryGetValue(collider2D.transform)
            if let Some(p) = self.physical_of_collider(collider).filter(|&p| self.collider_on_physical_object(collider, p) && self.physicals[p].in_world) {
                self.physicals[p].charge = self.physicals[p].Charge() + 2.0 * dt;
            }
        }
        let x = &mut self.lightning_entities[e];
        if x.cooldown > 0.0 {
            x.cooldown -= self.time.unscaledDeltaTime;
        // Vector3.sqrMagnitude, in double and rounded at its return.
        } else if (d(x.delta[0]) * x.delta[0] + d(x.delta[1]) * x.delta[1] + d(x.delta[2]) * x.delta[2]).f() > x.BoltThreshold {
            self.lightning_fire_bolt(e);
        }
    }

    /// LightningToolEntityBehaviour.FireBolt up to its first yield.
    pub(crate) fn lightning_fire_bolt(&mut self, e: usize) {
        let id = self.lightning_entities[e].id;
        let position = self.with_component_instance(id, |_, inst, o| inst.position(o));
        let delta = self.lightning_entities[e].delta;
        let Some(hit) = self.raycast(V::new(position[0], position[1]), [delta[0], delta[1]], 5000.0, MASK, f32::NEG_INFINITY, f32::INFINITY) else { return };
        // x = Random.value * 10000f: the Perlin seed of the line; num = distance * 0.2f.
        let seed = self.random.value() * 10000.0;
        let num = hit.distance * 0.2;
        let x = &mut self.lightning_entities[e];
        x.cooldown = x.CooldownTime;
        let (thunder, vertices) = (x.thunder, x.vertices);
        // AudioSource.PlayOneShot(Thunder.PickRandom())
        let id = self.lightning_entities[e].id;
        let clip = self.pick_clip(id, "Thunder", thunder);
        let src = self.lightning_source(id, "AudioSource");
        self.audio.play_one_shot(src, clip.as_ref(), 1.0);
        // Each vertex: GetPerlin2Mapped (no Random) + Random.insideUnitCircle * 0.2f; then SingleBolt.SetPositions.
        let circles: Vec<[f32; 2]> = (0..vertices).map(|_| self.random.inside_unit_circle()).collect();
        if let Some(l) = self.lightning_entities[e].bolt {
            self.lines[l].positions = lightning_bolt_points(position, [hit.point.x, hit.point.y], num, seed, &circles);
        }
        let point = [hit.point.x, hit.point.y, 0.0];
        let water = self.is_point_under_water([point[0], point[1]]);
        self.create_explosion_with_water(water, ExplosionParameters {
            FragmentationRayCount: 8,
            Position: point,
            Range: 15.0,
            FragmentForce: 8.0,
            CreateParticlesAndSound: true,
            EffectSize: EffectSize::Large,
            DismemberChance: 0.0,
            BallisticShrapnelCount: 0,
        });
        if let Some(p) = self.hit_world_physical(&hit) {
            self.physicals[p].charge = self.physicals[p].Charge() + 150.0;
        }
        self.lightning_flash_loop(e, 0);
    }

    /// FireBolt's `for (j = 0; j < Random.Range(2, 5); j++)` check, then the flash's WaitForSeconds(0.032f).
    fn lightning_flash_loop(&mut self, e: usize, j: i32) {
        if j < self.random.range_int(2, 5) {
            self.lightning_entity_show(e, true);
            let id = self.lightning_entities[e].id;
            self.start_wait(id, 0.032, Step::LightningFlash(e, j));
        }
    }

    /// FireBolt after a flash: WaitForSeconds(Random.Range(0.032f, 0.3f)).
    pub(super) fn lightning_flash_end(&mut self, e: usize, j: i32) {
        self.lightning_entity_show(e, false);
        let wait = self.random.range(0.032, 0.3);
        let id = self.lightning_entities[e].id;
        self.start_wait(id, wait, Step::LightningGap(e, j));
    }

    /// FireBolt after the gap: j++ and the loop check.
    pub(super) fn lightning_gap_end(&mut self, e: usize, j: i32) {
        self.lightning_flash_loop(e, j + 1);
    }
}
