//! ChainsawBehaviour. FixedUpdate decays the rotation speed, saws what the SawCollider overlaps (soft connections,
//! Slice and Shot messages, pushes) and drives the saw with the rotating ForceVector; Update adds speed while it is
//! used continuously, and moves the chain (`_Position` of the Saw's Unlit/ChainsawShader, custom_material.rs). Start plays
//! the IdleLoop and FixedUpdate the RevLoop (AudioSources); the overcharge particles and the saw sprite's jitter are
//! rendering (the jitter is Perlin noise, no draws); Use only plays a clip.
#![allow(non_snake_case)]

use super::soft_connect::SoftConnections;
use super::{d, mathf, script, Game, D};

/// The buffer's length (`new Collider2D[8]`): OverlapCollider keeps its first 8 hits.
const BUFFER: usize = 8;

script! {
    Chainsaw {
        RotationSpeedDecay: f32 = 0.9,
        UseRotationSpeedAddition: f32 = 15.0,
        JitterIntensity: f32 = 0.005,
        JitterSpeed: f32 = 0.1,
        MinSoftness: f32 = 0.5,
        SpeedCutThreshold: f32 = 1.0,
        Damage: f32 = 25.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// PhysicalBehaviour
        physical: usize = 0,
        /// SawCollider: (object, component) in its instance, and its instance id.
        SawCollider: (usize, usize) = (0, 0),
        saw_collider_id: i32 = 0,
        /// IdleLoop and RevLoop: AudioSource instance ids.
        IdleLoop: i32 = 0,
        RevLoop: i32 = 0,
        /// DamagableMachinery (a `machinery` index).
        DamagableMachinery: usize = 0,
        /// LayerMask.m_Bits
        LayerMask: u32 = 0,
        rotationSpeed: f32 = 0.0,
        seed: f32 = 0.0,
        /// Saw
        Saw: super::visual::SpriteAt = super::visual::SpriteAt::Renderer(0),
        chainPos: f32 = 0.0,
        /// propertyBlock, read from the Saw in Start.
        propertyBlock: super::props::PropBlock = super::props::PropBlock::default(),
        softConnections: SoftConnections = SoftConnections::default(),
        /// SerialisableVictims: the Guid[] OnBeforeSerialise fills and the paste injects.
        SerialisableVictims: Vec<u128> = Vec::new(),
    }
}

impl Game {
    /// ChainsawBehaviour.ForceVector
    /// ForceVector: (Charge + 1) * 2 * (cos f, sin f) * rotationSpeed, f = Time.time * -(85 + Charge).
    pub(crate) fn chainsaw_force_vector(&self, i: usize) -> [f32; 2] {
        let charge = self.physicals[self.chainsaws[i].physical].Charge();
        let f = (d(self.time.time) * (D(0.0) - (85.0f32 + d(charge)))).f();
        let k = ((d(charge) + 1.0f32) * 2.0f32).f();
        let v = [k * mathf::cos(f), k * mathf::sin(f)];
        let s = self.chainsaws[i].rotationSpeed;
        [v[0] * s, v[1] * s]
    }

    /// ChainsawBehaviour.OnBeforeSerialise
    pub(super) fn chainsaw_on_before_serialise(&mut self, i: usize) -> Vec<u128> {
        let conns = self.chainsaws[i].softConnections.clone();
        let guids = self.soft_connection_guids(&conns);
        self.chainsaws[i].SerialisableVictims = guids.clone();
        guids
    }

    /// ChainsawBehaviour.OnAfterDeserialise: SoftConnect(component, component.GetComponent<Collider2D>(), 2f) to each victim that exists among the
    /// new objects; the others only log a warning.
    pub(crate) fn chainsaw_on_after_deserialise(&mut self, i: usize, objects: &[i32]) {
        for guid in self.chainsaws[i].SerialisableVictims.clone() {
            let Some((v, coll)) = self.deserialised_victim(guid, objects) else { continue };
            self.chainsaw_soft_connect(i, Some(v), Some(coll), 2.0);
        }
    }

    /// ChainsawBehaviour.SoftConnect(otherPhys, coll, connectionStrength): `if (otherPhys && coll)`, the joint, the ignore and the Add.
    pub(crate) fn chainsaw_soft_connect(&mut self, i: usize, other: Option<usize>, coll: Option<i32>, strength: f32) {
        let (Some(other), Some(coll)) = (other, coll) else { return };
        let (p, saw_collider) = (self.chainsaws[i].physical, self.chainsaws[i].saw_collider_id);
        let mut conns = std::mem::take(&mut self.chainsaws[i].softConnections);
        self.soft_connect(&mut conns, p, saw_collider, other, coll, strength, false, &mut |g| g.saw_hit_point(p, saw_collider, coll));
        self.chainsaws[i].softConnections = conns;
    }

    /// ChainsawBehaviour.Start: the seed's draw and the Saw's property block (the idle loop is audio).
    pub fn chainsaw_start(&mut self, i: usize) {
        self.chainsaws[i].seed = self.random.value() * 100.0;
        self.chainsaws[i].propertyBlock = self.get_property_block(self.chainsaws[i].Saw);
        if !self.machinery[self.chainsaws[i].DamagableMachinery].Destroyed {
            self.audio.play(self.chainsaws[i].IdleLoop);
        }
    }

    /// ChainsawBehaviour.FixedUpdate
    pub fn chainsaw_fixed_update(&mut self, i: usize) {
        let s = &mut self.chainsaws[i];
        s.rotationSpeed *= s.RotationSpeedDecay;
        let flag = s.rotationSpeed > s.SpeedCutThreshold;
        let (rev, speed) = (s.RevLoop, s.rotationSpeed);
        if speed > 0.1 && !self.audio.is_playing(rev) {
            self.audio.play(rev);
        } else if speed <= 0.1 && self.audio.is_playing(rev) {
            self.audio.stop(rev);
        }
        if self.audio.is_playing(rev) {
            self.audio.get_mut(rev).volume = mathf::clamp01(speed);
        }
        self.chainsaw_process(i, flag, if flag { 2.0 } else { 500.0 });
        let (id, key) = (self.chainsaws[i].id, self.chainsaws[i].SawCollider);
        self.with_component_instance(id, |g, inst, _| g.collider_set_is_trigger(inst, key, false));
        // OverchargeSystem plays while sawing above 1 of charge and stops when not sawing or at 5 or less (by isPlaying).
        let charge = self.physicals[self.chainsaws[i].physical].Charge();
        let playing = self.field_ps(id, "OverchargeSystem").is_some_and(|k| crate::particles::playing(self, k));
        if flag && charge > 1.0 && !playing {
            self.ps_field_play_stop(id, "OverchargeSystem", true);
        } else if (!flag || charge <= 5.0) && playing {
            self.ps_field_play_stop(id, "OverchargeSystem", false);
        }
        let force = self.chainsaw_force_vector(i);
        let rb = self.physicals[self.chainsaws[i].physical].rigidbody.clone();
        rb.add_relative_force(&mut self.world, force);
    }

    /// ChainsawBehaviour.ProcessSoftConnections(shouldSaw, connectionStrength)
    pub(crate) fn chainsaw_process(&mut self, i: usize, should_saw: bool, strength: f32) {
        let s = &self.chainsaws[i];
        let (id, key, saw_collider, p, mask) = (s.id, s.SawCollider, s.saw_collider_id, s.physical, s.LayerMask);
        // OverlapCollider's AutoSyncTransforms first: a flip's sync re-creates the collider's shapes.
        self.sync_pending();
        let fixtures = self.with_component_instance(id, |_, inst, _| inst.fixtures.get(&key).cloned().unwrap_or_default());
        let mut hits = self.overlap_collider(&fixtures, mask);
        hits.truncate(BUFFER);
        let mut conns = std::mem::take(&mut self.chainsaws[i].softConnections);
        for k in conns.keys() {
            conns.get_mut(k).unwrap().should_be_deleted = true;
        }
        let force = self.chainsaw_force_vector(i);
        let relative = [force[0] * 0.5, force[1] * 0.5];
        let num = self.physicals[p].Charge() * 0.1;
        let my_root = self.root_id(p);
        for (victim, coll) in hits {
            // collider2D.transform.root != base.transform.root, then PhysicalObjectsInWorldByTransform.TryGetValue.
            let Some(v) = victim.filter(|&v| self.root_id(v) != my_root && self.physicals[v].in_world) else { continue };
            let rb = self.physicals[v].rigidbody.clone();
            rb.add_relative_force(&mut self.world, relative);
            if let Some(c) = conns.get(v).copied() {
                let broken = self.soft_connection_broken(&c);
                conns.get_mut(v).unwrap().should_be_deleted = broken;
                if broken {
                    continue;
                }
                let hit = self.saw_hit_point(p, saw_collider, coll);
                let a = self.inverse_transform_point(c.phys, [hit[0], hit[1], 0.0]);
                self.fixed_joint_set_anchor(c.joint, [a[0], a[1]]);
                self.friction_joint_set_max_force(c.joint, strength);
                self.friction_joint_set_max_torque(c.joint, strength);
                if should_saw {
                    if d(self.random.value()) > d(0.995) - num {
                        self.broadcast_slice(v);
                    } else if self.random.value() > 0.85 {
                        let n = self.saw_direction(p, hit);
                        let damage = self.chainsaws[i].Damage;
                        self.broadcast_saw_shot(v, coll, [n[0], n[1]], hit, damage);
                    }
                }
            } else {
                if !should_saw {
                    continue;
                }
                let softness = self.properties[self.physicals[v].Properties].Softness;
                if softness >= self.chainsaws[i].MinSoftness {
                    self.soft_connect(&mut conns, p, saw_collider, v, coll, strength, false, &mut |g| g.saw_hit_point(p, saw_collider, coll));
                    continue;
                }
                let hit = self.saw_hit_point(p, saw_collider, coll);
                let n = self.saw_direction(p, hit);
                let k = ((1.0f32 - d(softness)) * 50.0f32).f();
                let rb = self.physicals[p].rigidbody.clone();
                rb.add_force_at_position(&mut self.world, [k * n[0], k * n[1]], hit);
                if self.random.value() > 0.85 {
                    let damage = self.chainsaws[i].Damage;
                    self.broadcast_saw_shot(v, coll, [n[0], n[1]], hit, damage);
                }
            }
        }
        self.soft_connections_prune(&mut conns, saw_collider);
        self.chainsaws[i].softConnections = conns;
    }

    /// ChainsawBehaviour.Update: the chain moves by the speed, then the speed builds while it is used continuously and not destroyed.
    pub fn chainsaw_update(&mut self, i: usize) {
        // A destroyed machine stops a playing IdleLoop; a working one plays it when it is not playing.
        let (idle, destroyed) = (self.chainsaws[i].IdleLoop, self.machinery[self.chainsaws[i].DamagableMachinery].Destroyed);
        if destroyed && self.audio.is_playing(idle) {
            self.audio.stop(idle);
        } else if !destroyed && !self.audio.is_playing(idle) {
            self.audio.play(idle);
        }
        // Saw.transform.localPosition = JitterIntensity * Max(rotationSpeed, Destroyed ? 0 : 0.5) * GetPerlin2Mapped(num, -num + seed), num = Time.time * JitterSpeed
        let s = &self.chainsaws[i];
        let num = (d(self.time.time) * s.JitterSpeed).f();
        let floor = if destroyed { 0.0 } else { 0.5 };
        let k = (d(s.JitterIntensity) * if s.rotationSpeed > floor { s.rotationSpeed } else { floor }).f();
        let [px, py] = mathf::perlin2_mapped(num, (d(0.0f32 - num) + s.seed).f());

        let saw = s.Saw;
        self.set_sprite_local_position(saw, [(d(px) * k).f(), (d(py) * k).f(), (d(0.0f32) * k).f()]);
        // chainPos += rotationSpeed * Time.deltaTime; propertyBlock.SetFloat("_Position", chainPos); Saw.SetPropertyBlock(propertyBlock)
        let s = &mut self.chainsaws[i];

        s.chainPos = (d(s.chainPos) + d(s.rotationSpeed) * self.time.deltaTime).f();
        let mut block = std::mem::take(&mut s.propertyBlock);
        block.set_float("_Position", s.chainPos);
        let saw = s.Saw;
        self.set_property_block(saw, &block);
        self.chainsaws[i].propertyBlock = block;
        let s = &self.chainsaws[i];
        if self.used_continuously(s.physical) && !self.machinery[s.DamagableMachinery].Destroyed {
            let s = &mut self.chainsaws[i];
            s.rotationSpeed = (d(s.rotationSpeed) + d(self.time.deltaTime) * s.UseRotationSpeedAddition).f();
        }
    }

    /// ChainsawBehaviour.OnDestroy: Object.Destroy of every connection's joint.
    pub fn chainsaw_destroy(&mut self, i: usize) {
        for k in self.chainsaws[i].softConnections.keys() {
            let f = self.chainsaws[i].softConnections.get(k).unwrap().joint;
            if self.soft_joint_alive(f) {
                self.destroy_joint_delayed(f);
            }
        }
    }
}
