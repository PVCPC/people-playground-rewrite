//! EnergySwordBehaviour. Use switches the blade (BladeObject and MotionBlur) on and off; FixedUpdate cuts what the
//! BladeCollider overlaps through soft connections (soft_connect.rs), heating, burning and damaging what stays
//! connected and slicing it at random; Update cools the impact sound's cooldown and keeps the rain particles on while the
//! map rains. The loop, impact and rain audio are presentation; the impact loop's isPlaying is kept (its Play draws).
#![allow(non_snake_case)]

use super::soft_connect::SoftConnections;
use super::{d, mathf, script, Game};

/// The overlap buffer (`new Collider2D[16]`) and the cast buffer (`new RaycastHit2D[16]`).
pub(crate) const BUFFER: usize = 16;

script! {
    EnergySword {
        SpeedCutThreshold: f32 = 1.0,
        MinSoftness: f32 = 0.0,
        Damage: f32 = 256.0,
        BladeTemperature: f32 = 900.0,
        /// 1.28.57: `public Color Color = Color.red` (1.28.41's prefab has none; its default is never read there).
        Color: [f32; 4] = [1.0, 0.0, 0.0, 1.0],
        Activated: bool = false,
    }
    links {
        /// 1.28.57's [SkipSerialisation] LightSprite, BladeRenderer, BladeOverlayRenderer (SpriteRenderers), and
        /// MotionBlurRenderer (EnergySwordMotionBlur) and ClashBehaviour (EnergySwordBladeClashBehaviour) by instance id.
        LightSprite: Option<super::visual::SpriteAt> = None,
        BladeRenderer: Option<super::visual::SpriteAt> = None,
        BladeOverlayRenderer: Option<super::visual::SpriteAt> = None,
        MotionBlurRenderer: i32 = 0,
        ClashBehaviour: i32 = 0,
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// PhysicalBehaviour
        physical: usize = 0,
        /// MainCollider's instance id.
        MainCollider: i32 = 0,
        /// BladeCollider: (object, component) in its instance, and its instance id.
        BladeCollider: (usize, usize) = (0, 0),
        blade_collider_id: i32 = 0,
        /// LocalBladeLine.A and .B
        LocalBladeLine: [[f32; 2]; 2] = [[0.0; 2]; 2],
        /// BladeObject and MotionBlur: objects of the instance.
        BladeObject: usize = 0,
        MotionBlur: usize = 0,
        /// LayerMask.m_Bits
        LayerMask: u32 = 0,
        /// ImpactSound.Length
        ImpactSound: usize = 0,
        /// ImpactSource.isPlaying (a looping AudioSource plays until Stop).
        impact_playing: bool = false,
        /// LoopSource: its instance id (SetActivation plays and stops it).
        LoopSource: i32 = 0,
        /// currentMapConfig != null
        map_config: bool = false,
        impactSoundHeat: f32 = 0.0,
        softConnections: SoftConnections = SoftConnections::default(),
        /// SerialisableVictims: the Guid[] OnBeforeSerialise fills and the paste injects.
        SerialisableVictims: Vec<u128> = Vec::new(),
    }
}

// EnergySwordBehaviour.Awake: BladeObject.SetActive(false) and MotionBlur.SetActive(false); Start's SetActivation sets both to Activated before any step
/// EnergySwordMotionBlur.Node
#[derive(Clone, Copy, Debug)]
pub struct BlurNode {
    pub Life: f32,
    pub A: [f32; 3],
    pub B: [f32; 3],
}

script! {
    /// EnergySwordMotionBlur: the blade's trail, a mesh of the last blade positions (rendered by src/energy_blur.rs).
    EnergySwordBlur {
        SendToBottomLayer: bool = false,
        MakeBottomColorTransparent: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// ColorMultiplier (1.28.57, [SkipSerialisation]): Color.white until EnergySwordBehaviour.SetColour.
        ColorMultiplier: [f32; 4] = [1.0; 4],
        /// Life, MinDistance and ColorOverMesh ([SkipSerialisation]).
        Life: f32 = 1.0,
        MinDistance: f32 = 0.1,
        ColorOverMesh: super::visual::Gradient = Default::default(),
        /// A and B: their anchor and the chain of local transforms below it.
        A: (super::BareAnchor, Vec<crate::unity::transform::Trs>) = (super::BareAnchor::Physical(0), Vec::new()),
        B: (super::BareAnchor, Vec<crate::unity::transform::Trs>) = (super::BareAnchor::Physical(0), Vec::new()),
        /// MeshRenderer: its instance id, enabled, first material, sorting layer id (Awake's "Bottom" with
        /// SendToBottomLayer) and order, and its GameObject's layer and activeInHierarchy.
        renderer_id: i32 = 0,
        renderer_enabled: bool = true,
        material: Option<crate::assets::db::ObjRef> = None,
        sorting_layer: i32 = 0,
        sorting_order: i16 = 0,
        game_layer: i32 = 0,
        visible: bool = true,
        nodes: Vec<BlurNode> = Vec::new(),
        /// The mesh: vertices, colours and triangle indices as last set; `version` counts the sets.
        vertices: Vec<[f32; 3]> = Vec::new(),
        colors: Vec<[f32; 4]> = Vec::new(),
        indices: Vec<u32> = Vec::new(),
        version: u32 = 0,
    }
}

impl Game {
    /// EnergySwordBehaviour.OnBeforeSerialise
    pub(super) fn energy_sword_on_before_serialise(&mut self, i: usize) -> Vec<u128> {
        let conns = self.energy_swords[i].softConnections.clone();
        let guids = self.soft_connection_guids(&conns);
        self.energy_swords[i].SerialisableVictims = guids.clone();
        guids
    }

    /// EnergySwordBehaviour.OnAfterDeserialise: SoftConnect(component, component.GetComponent<Collider2D>(), 2f) to each victim that exists among the
    /// new objects; the others only log a warning.
    pub(crate) fn energy_sword_on_after_deserialise(&mut self, i: usize, objects: &[i32]) {
        let (p, blade) = (self.energy_swords[i].physical, self.energy_swords[i].blade_collider_id);
        for guid in self.energy_swords[i].SerialisableVictims.clone() {
            let Some((v, coll)) = self.deserialised_victim(guid, objects) else { continue };
            let mut conns = std::mem::take(&mut self.energy_swords[i].softConnections);
            self.soft_connect(&mut conns, p, blade, v, coll, 2.0, false, &mut |g| {
                if g.energy_swords[i].impactSoundHeat <= f32::from_bits(1) {
                    // LoopSource.PlayOneShot(ImpactSound.PickRandom(), 1.5f)
                    // fc41 r5: the clip is played (1.28.52's sword has ImpactSound clips); an empty array picks null, which only logs.
                    let (id, src) = (g.energy_swords[i].id, g.energy_swords[i].LoopSource);
                    let clip = g.pick_clip(id, "ImpactSound", g.energy_swords[i].ImpactSound);
                    g.audio.play_one_shot(src, clip.as_ref(), 1.5);
                    g.energy_swords[i].impactSoundHeat = 0.5;
                }
                g.energy_sword_hit_point(i, coll)
            });
            self.energy_swords[i].softConnections = conns;
        }
    }

    /// EnergySwordBehaviour.Start: FindObjectOfType<MapConfig>, then SetActivation(playActivationSounds: false).
    /// 1.28.57: first the "Change colour" context button (its condition, !ColorpickerDialogBehaviour.IsOpen, holds: no
    /// dialog is open while a menu is); currentMapConfig = MapConfig.Instance; last SetColour(Color).
    pub fn energy_sword_start(&mut self, i: usize) {
        let new = crate::assets::il::is_1_28_45();
        if new {
            let p = self.energy_swords[i].physical;
            self.physicals[p].menu.push(super::menu::MenuButton::new("setColour", crate::il45!(str "EnergySwordBehaviour::Start" 1), crate::il45!(str "EnergySwordBehaviour::Start" 2), super::menu::MenuAction::Entry(super::menu::Entry::BladeColour(i))));
        }
        self.energy_swords[i].map_config = self.map_config_present;
        self.energy_sword_set_activation(i, false);
        if new {
            let c = self.energy_swords[i].Color;
            self.energy_sword_set_colour(i, c);
        }
    }

    /// EnergySwordBehaviour.SetColour (1.28.57): alpha 1; Color, LightSprite and BladeOverlayRenderer take it, BladeRenderer a
    /// quarter of it (alpha 1), the motion blur's ColorMultiplier and the clash's Color; then the startColor of each
    /// ParticleSystem under HitParticle not on the TransparentFX layer.
    pub(crate) fn energy_sword_set_colour(&mut self, i: usize, mut color: [f32; 4]) {
        color[3] = 1.0;
        let s = &mut self.energy_swords[i];
        s.Color = color;
        let (light, blade, overlay, blur, clash, id) = (s.LightSprite, s.BladeRenderer, s.BladeOverlayRenderer, s.MotionBlurRenderer, s.ClashBehaviour, s.id);
        let null = |x: Option<super::visual::SpriteAt>| x.unwrap_or_else(|| super::menu::throw("UnassignedReferenceException: EnergySwordBehaviour.SetColour"));
        self.set_sprite_color(null(light), color);
        self.set_sprite_color(null(blade), [color[0] * 0.25, color[1] * 0.25, color[2] * 0.25, 1.0]);
        self.set_sprite_color(null(overlay), color);
        let b = self.energy_blurs.iter().position(|b| b.id == blur).unwrap_or_else(|| super::menu::throw("UnassignedReferenceException: EnergySwordBehaviour.MotionBlurRenderer"));
        self.energy_blurs[b].ColorMultiplier = color;
        let k = self.blade_clashes.iter().position(|x| x.0 == clash).unwrap_or_else(|| super::menu::throw("UnassignedReferenceException: EnergySwordBehaviour.ClashBehaviour"));
        self.blade_clashes[k].3 = color;
        self.ps_field_start_color_opaque(id, "HitParticle", color);
    }

    /// EnergySwordBehaviour.Use
    pub fn energy_sword_use(&mut self, i: usize) {
        if !self.energy_swords[i].enabled {
            return;
        }
        self.energy_swords[i].Activated = !self.energy_swords[i].Activated;
        self.energy_sword_set_activation(i, true);
        if self.energy_swords[i].Activated {
            // AddRelativeForce(Vector2.down * 0.1f, ForceMode2D.Impulse)
            let rb = self.physicals[self.energy_swords[i].physical].rigidbody.clone();
            rb.add_relative_force_impulse(&mut self.world, [0.0, -0.1]);
        }
    }

    /// SetActivation on the clone that holds the sword.
    fn energy_sword_set_activation(&mut self, i: usize, play_activation_sounds: bool) {
        let id = self.energy_swords[i].id;
        self.with_component_instance(id, |g, inst, _| g.energy_sword_set_activation_in(i, inst, play_activation_sounds));
        // BoilingVFX.Play(withChildren: true); off: HitParticle, BoilingVFX and RainParticles Stop. (OnDisable's
        // SetActivation runs with the clone taken out, where the fields are not found: the object goes away then.)
        if self.energy_swords[i].Activated {
            self.ps_field_play_stop(id, "BoilingVFX", true);
        } else {
            for f in ["HitParticle", "BoilingVFX", "RainParticles"] {
                self.ps_field_play_stop(id, f, false);
            }
        }
    }

    /// EnergySwordBehaviour.SetActivation: MotionBlur and BladeObject follow Activated; switching off stops the impact
    /// loop and destroys every connection's joint (the entries stay until the next FixedUpdate prunes them).
    pub fn energy_sword_set_activation_in(&mut self, i: usize, inst: &mut super::spawn::Instance, play_activation_sounds: bool) {
        let s = &self.energy_swords[i];
        let (on, blur, blade, id) = (s.Activated, s.MotionBlur, s.BladeObject, s.id);
        // `if ((bool)MotionBlur)`: OnDestroy destroys it, and it can be detached and destroyed with its parent first.
        if !self.destroyed_components.contains(&inst.objects[blur].components[0]) {
            self.set_active_scripts(inst, blur, on);
        }
        self.set_active_scripts(inst, blade, on);
        let loop_source = self.energy_swords[i].LoopSource;
        if on {
            self.audio.play(loop_source);
            if play_activation_sounds {
                let clip = self.audio.clip(id, "PowerOn");
                self.audio.play_one_shot(loop_source, clip.as_ref(), 2.0);
            }
            return;
        }
        self.audio.stop(loop_source);
        self.energy_swords[i].impact_playing = false;
        for k in self.energy_swords[i].softConnections.keys() {
            let f = self.energy_swords[i].softConnections.get(k).unwrap().joint;
            if self.soft_joint_alive(f) {
                self.destroy_joint_delayed(f);
            }
        }
        if play_activation_sounds {
            let clip = self.audio.clip(id, "PowerOff");
            self.audio.play_one_shot(loop_source, clip.as_ref(), 2.0);
        }
    }

    /// EnergySwordBehaviour.OnDisable
    pub fn energy_sword_on_disable(&mut self, i: usize, inst: &mut super::spawn::Instance) {
        if self.energy_swords[i].Activated {
            self.energy_swords[i].Activated = false;
            self.energy_sword_set_activation_in(i, inst, true);
        }
    }

    /// EnergySwordBehaviour.Update: RainParticles plays while the map rains (Settings.Rain and a RainPrefab), by
    /// isEmitting; then the impact sound's cooldown.
    pub fn energy_sword_update(&mut self, i: usize) {
        if !self.energy_swords[i].Activated {
            return;
        }
        if !self.energy_swords[i].map_config {
            crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        }
        let rain = self.map_settings.is_some_and(|m| m.Rain) && self.map_config.as_ref().is_some_and(|m| m.weather[1].is_some());
        self.ps_field_emit(self.energy_swords[i].id, "RainParticles", rain);
        let s = &mut self.energy_swords[i];
        s.impactSoundHeat -= self.time.deltaTime;
        if s.impactSoundHeat < f32::from_bits(1) {
            s.impactSoundHeat = 0.0;
        }
    }

    /// EnergySwordBehaviour.FixedUpdate: ProcessSoftConnections(shouldCut: true, 15f).
    pub fn energy_sword_fixed_update(&mut self, i: usize) {
        self.energy_sword_process(i, true, 15.0);
    }

    /// EnergySwordBehaviour.GetHitPoint: the blade line's circle cast hit on `coll`, else BladeCollider's closest
    /// point to coll's closest point to the sword.
    pub(crate) fn energy_sword_hit_point(&mut self, i: usize, coll: i32) -> [f32; 2] {
        let s = &self.energy_swords[i];
        let (p, line, blade) = (s.physical, s.LocalBladeLine, s.blade_collider_id);
        let a = self.transform_point3(p, [line[0][0], line[0][1], 0.0]);
        let b = self.transform_point3(p, [line[1][0], line[1][1], 0.0]);
        let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let magnitude = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let mut hits = self.circle_cast_all([a[0], a[1]], 0.05, [v[0], v[1]], magnitude, super::explosion::DEFAULT_RAYCAST_LAYERS);
        hits.truncate(BUFFER);
        if let Some(h) = hits.iter().find(|h| h.collider == coll) {
            return [h.point.x, h.point.y];
        }
        self.saw_hit_point(p, blade, coll)
    }

    /// EnergySwordBehaviour.ProcessSoftConnections(shouldCut, connectionStrength)
    pub(crate) fn energy_sword_process(&mut self, i: usize, should_cut: bool, strength: f32) {
        let s = &self.energy_swords[i];
        let (id, key, blade, main, p, mask) = (s.id, s.BladeCollider, s.blade_collider_id, s.MainCollider, s.physical, s.LayerMask);
        // OverlapCollider's AutoSyncTransforms first: a flip's sync re-creates the collider's shapes.
        self.sync_pending();
        let fixtures = self.with_component_instance(id, |_, inst, _| inst.fixtures.get(&key).cloned().unwrap_or_default());
        let mut hits = self.overlap_collider(&fixtures, mask);
        hits.truncate(BUFFER);
        let mut conns = std::mem::take(&mut self.energy_swords[i].softConnections);
        for k in conns.keys() {
            conns.get_mut(k).unwrap().should_be_deleted = true;
        }
        let num2 = self.physicals[p].Charge() * 0.1;
        let num3 = self.time.fixedDeltaTime * 0.02;
        let mut flag = false;
        let my_root = self.root_id(p);
        for (victim, coll) in hits {
            // collider2D.transform.root != base.transform.root, then PhysicalObjectsInWorldByTransform.TryGetValue.
            let Some(v) = victim.filter(|&v| self.root_id(v) != my_root && self.physicals[v].in_world) else { continue };
            if let Some(c) = conns.get(v).copied() {
                let broken = self.soft_connection_broken(&c);
                conns.get_mut(v).unwrap().should_be_deleted = broken;
                if broken {
                    continue;
                }
                flag = true;
                let hit = self.energy_sword_hit_point(i, coll);
                let a = self.inverse_transform_point(c.phys, [hit[0], hit[1], 0.0]);
                self.fixed_joint_set_anchor(c.joint, [a[0], a[1]]);
                self.friction_joint_set_max_force(c.joint, strength);
                self.friction_joint_set_max_torque(c.joint, strength);
                if self.physicals[c.phys].SimulateTemperature {
                    let t = self.physicals[c.phys].Temperature;
                    let target = self.energy_swords[i].BladeTemperature;
                    self.physicals[c.phys].Temperature = mathf::lerp(t, target, 0.00025 / self.heat_capacity(c.phys));
                }
                self.physicals[c.phys].BurnProgress += num3;
                let damage = self.energy_swords[i].Damage * self.time.fixedDeltaTime;
                self.send_damage(c.phys, damage);
                // HitParticle.transform.position = position
                if let Some(t) = self.field_component(id, "HitParticle") {
                    self.with_component_instance(t, |_, inst, o| inst.set_world_position(o, [hit[0], hit[1], 0.0]));
                }
                if should_cut && d(self.random.value()) > d(0.995) - num2 {
                    self.broadcast_slice(v);
                }
            } else if should_cut && !self.world.ignored.contains(&(coll.min(main), coll.max(main))) {
                self.soft_connect(&mut conns, p, blade, v, coll, strength, false, &mut |g| {
                    if g.energy_swords[i].impactSoundHeat <= f32::from_bits(1) {
                        // LoopSource.PlayOneShot(ImpactSound.PickRandom(), 1.5f)
                        // fc41 r5: the clip is played (1.28.52's sword has ImpactSound clips); an empty array picks null, which only logs.
                        let (id, src) = (g.energy_swords[i].id, g.energy_swords[i].LoopSource);
                        let clip = g.pick_clip(id, "ImpactSound", g.energy_swords[i].ImpactSound);
                        g.audio.play_one_shot(src, clip.as_ref(), 1.5);
                        g.energy_swords[i].impactSoundHeat = 0.5;
                    }
                    g.energy_sword_hit_point(i, coll)
                });
            }
        }
        // gapC r11: HitParticle played (Activated) or stopped by its isPlaying, before the ImpactSource.
        let id = self.energy_swords[i].id;
        if flag {
            if self.energy_swords[i].Activated {
                self.ps_field_playing(id, "HitParticle", true);
            }
            // ImpactSource.Play(), then ImpactSource.time = clip.length * Random.value.
            if self.energy_swords[i].Activated && !self.energy_swords[i].impact_playing {
                self.energy_swords[i].impact_playing = true;
                self.random.value();
            }
        } else {
            self.ps_field_playing(id, "HitParticle", false);
            self.energy_swords[i].impact_playing = false;
        }
        self.soft_connections_prune(&mut conns, blade);
        self.energy_swords[i].softConnections = conns;
    }

    /// EnergySwordBehaviour.OnDestroy: Destroy(MotionBlur), then each connection collides with the blade again and
    /// its joint is destroyed.
    pub fn energy_sword_destroy(&mut self, i: usize, blur: i32) {
        self.destroy_object(blur);
        let blade = self.energy_swords[i].blade_collider_id;
        for k in self.energy_swords[i].softConnections.keys() {
            let c = *self.energy_swords[i].softConnections.get(k).unwrap();
            self.ignore_collision_substitute(c.coll, blade, false);
            if self.soft_joint_alive(c.joint) {
                self.destroy_joint_delayed(c.joint);
            }
        }
    }

    /// EnergySwordMotionBlur.OnEnable: mesh.Clear() and nodes.Clear(). The mesh is made in Awake, so a blur whose
    /// GameObject was never active (never awoken) throws on the null mesh.
    pub fn energy_blur_on_enable(&mut self, i: usize) {
        let awake = self.with_component_instance(self.energy_blurs[i].id, |_, inst, o| inst.objects[o].activated);
        self.energy_blur_on_enable_awake(i, awake);
    }

    /// EnergySwordMotionBlur.OnEnable, `awake`: its Awake ran (the Mesh it clears exists).
    pub(crate) fn energy_blur_on_enable_awake(&mut self, i: usize, awake: bool) {
        if !awake {
            super::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        }
        let b = &mut self.energy_blurs[i];
        b.vertices.clear();
        b.colors.clear();
        b.indices.clear();
        b.version += 1;
        b.nodes.clear();
    }

    /// EnergySwordMotionBlur.Update: its own transform goes to the origin unrotated and unscaled (the mesh is in world
    /// space; nothing else reads that transform), a node is added when the blade's centre moved MinDistance from the last
    /// one's, then UpdateNodes and UpdateMesh.
    pub fn energy_blur_update(&mut self, i: usize) {
        // transform.SetPositionAndRotation(Vector3.zero, Quaternion.identity); transform.localScale = Vector3.one (gapB r11:
        // the GameObject's local pose under the sword, which its Clone's localRotation shows).
        let id = self.energy_blurs[i].id;
        self.with_component_instance(id, |_, inst, o| {
            inst.set_world_position(o, [0.0; 3]);
            inst.set_world_rotation(o, [0.0, 0.0, 0.0, 1.0]);
            inst.objects[o].trs.s = [1.0, 1.0, 1.0, inst.objects[o].trs.s[3]];
        });
        let num = self.energy_blurs[i].MinDistance * self.energy_blurs[i].MinDistance;
        let (a, b) = self.energy_blur_ends(i);
        let vector = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0];
        match self.energy_blurs[i].nodes.last().copied() {
            Some(node) => {
                let c = [(node.A[0] + node.B[0]) / 2.0, (node.A[1] + node.B[1]) / 2.0, (node.A[2] + node.B[2]) / 2.0];
                let d = [vector[0] - c[0], vector[1] - c[1], vector[2] - c[2]];
                if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] >= num {
                    self.energy_blur_create_node(i);
                }
            }
            None => self.energy_blur_create_node(i),
        }
        self.energy_blur_update_nodes(i);
        self.energy_blur_update_mesh(i);
    }

    /// A.position and B.position.
    fn energy_blur_ends(&self, i: usize) -> ([f32; 3], [f32; 3]) {
        let b = &self.energy_blurs[i];
        (self.anchored_world(b.A.0, &b.A.1).0, self.anchored_world(b.B.0, &b.B.1).0)
    }

    /// EnergySwordMotionBlur.GetCurrentBladePos: A.position and B.position as Vector2s (z dropped).
    pub(crate) fn energy_blur_current_blade_pos(&self, i: usize) -> ([f32; 3], [f32; 3]) {
        let (a, b) = self.energy_blur_ends(i);
        ([a[0], a[1], 0.0], [b[0], b[1], 0.0])
    }

    /// EnergySwordMotionBlur.CreateNode: below 128 nodes, one of the blade's ends with Life.
    pub(crate) fn energy_blur_create_node(&mut self, i: usize) {
        if self.energy_blurs[i].nodes.len() < 128 {
            let (a, b) = self.energy_blur_current_blade_pos(i);
            let life = self.energy_blurs[i].Life;
            self.energy_blurs[i].nodes.push(BlurNode { A: a, B: b, Life: life });
        }
    }

    /// EnergySwordMotionBlur.UpdateNodes: from the last, a node at or below 0 life goes, the others lose deltaTime.
    pub(crate) fn energy_blur_update_nodes(&mut self, i: usize) {
        let delta_time = self.time.deltaTime;
        let nodes = &mut self.energy_blurs[i].nodes;
        for num in (0..nodes.len()).rev() {
            if nodes[num].Life <= 0.0 {
                nodes.remove(num);
            } else {
                nodes[num].Life -= delta_time;
            }
        }
    }

    /// EnergySwordMotionBlur.UpdateMesh: with one node or none the MeshRenderer is disabled; else enabled, a vertex pair per
    /// node and one for the blade now, coloured by ColorOverMesh (the first pair clear, the B side clear with
    /// MakeBottomColorTransparent), and two triangles per step.
    pub(crate) fn energy_blur_update_mesh(&mut self, i: usize) {
        if self.energy_blurs[i].nodes.len() <= 1 {
            self.energy_blurs[i].renderer_enabled = false;
            return;
        }
        self.energy_blurs[i].renderer_enabled = true;
        let current = self.energy_blur_current_blade_pos(i);
        let b = &mut self.energy_blurs[i];
        let clear = [0.0f32; 4];
        // Color * Color: per channel.
        let mul = |a: [f32; 4], m: [f32; 4]| [a[0] * m[0], a[1] * m[1], a[2] * m[2], a[3] * m[3]];
        b.colors.clear();
        b.vertices.clear();
        let count = b.nodes.len();
        for k in 0..count {
            let num = (k as f32 + 1.0) / count as f32;
            let node = b.nodes[k];
            b.vertices.push(node.A);
            b.vertices.push(node.B);
            // 1.28.57: ColorOverMesh.Evaluate(1f - num) * ColorMultiplier (white on 1.28.41).
            let color = if k == 0 { clear } else { mul(b.ColorOverMesh.evaluate(1.0 - num), b.ColorMultiplier) };
            b.colors.push(color);
            b.colors.push(if b.MakeBottomColorTransparent { clear } else { color });
        }
        b.vertices.push(current.0);
        b.vertices.push(current.1);
        let color2 = mul(b.ColorOverMesh.evaluate(0.0), b.ColorMultiplier);
        b.colors.push(color2);
        b.colors.push(if b.MakeBottomColorTransparent { clear } else { color2 });
        b.indices.clear();
        let mut num2 = 0u32;
        while b.indices.len() < (b.vertices.len() - 2) * 3 {
            b.indices.extend([num2 + 1, num2 + 2, num2, num2 + 1, num2 + 2, num2 + 3]);
            num2 += 2;
        }
        // mesh.Clear(): the Mesh is made in Awake, which a never-active blur did not run.
        if !self.with_component_instance(self.energy_blurs[i].id, |_, inst, o| inst.objects[o].activated) {
            super::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        }
        self.energy_blurs[i].version += 1;
    }
}
