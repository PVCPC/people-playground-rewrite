//! IonBoltBehaviour (the Ion Cannon's bolt, a bare object without a PhysicalBehaviour). Update moves the bolt along its right
//! vector by Speed * deltaTime, unless a raycast of that step hits: an impact effect spawns, the bolt turns to the
//! reflection and stops on the hit point, a pulse explosion (no sound or effects) pushes what is around, every physical
//! object within AoERadius of the hit may break (a coin toss), is pushed by ImpactStrength and burns by 0.5 * Random.value,
//! the hit object takes a green decal and the DeleteAfterTime loses a tenth of its life. Start spawns an impact effect;
//! Awake's OnEndOfLife listener spawns the end-of-life effect and lets the Particles child fade for a second. The
//! effects' sounds are AUDIO call sites.
#![allow(non_snake_case)]

use super::translated::rt::Xf;
use super::{d, mathf, script, Game};
use crate::assets::db::ObjRef;
use crate::b2::math::Vec2 as V;

/// IonBoltBehaviour.buffer: Collider2D[16].
pub(crate) const BUFFER: usize = 16;

script! {
    IonBolt {
        Speed: f32 = 0.0,
        ImpactStrength: f32 = 0.0,
        DecalSize: f32 = 0.5,
        AoERadius: f32 = 1.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// Layers.m_Bits
        Layers: u32 = 0,
        /// DeleteAfterTime (`Game::delete_afters` index).
        delete_after: usize = 0,
        ImpactEffect: Option<ObjRef> = None,
        EndOfLifeEffect: Option<ObjRef> = None,
        /// ImpactDecal: an arena index of `Game::decal_descriptors`; None for a null asset.
        ImpactDecal: Option<usize> = None,
        /// Particles: the Transform id of its object.
        Particles: i32 = 0,
    }
}

impl Game {
    fn ion_bolt_impact_effect(&self, b: usize) -> ObjRef {
        self.ion_bolts[b].ImpactEffect.expect("IonBoltBehaviour: Instantiate(null ImpactEffect) (ArgumentException)")
    }

    /// IonBoltBehaviour.Start: Instantiate(ImpactEffect, transform.position, identity).transform.localScale = Vector3.one * 0.3f
    /// (the particles are drawn at that scale).
    pub fn ion_bolt_start(&mut self, b: usize) {
        let id = self.ion_bolts[b].id;
        let at = self.with_component_instance(id, |_, inst, o| inst.position(o));
        let effect = self.ion_bolt_impact_effect(b);
        let off = std::mem::replace(&mut self.effects_off, true);
        let root = self.spawn_prefab(effect, at);
        self.effects_off = off;
        self.with_component_instance(root, |_, inst, o| inst.set_local_scale(o, [0.3; 3]));
        self.log_effect(effect, at, [0.0, 0.0, 0.0, 1.0], 0.3, None, Some(root));
    }

    /// IonBoltBehaviour.Update
    pub fn ion_bolt_update(&mut self, b: usize) {
        let id = self.ion_bolts[b].id;
        let num = (d(self.ion_bolts[b].Speed) * self.time.deltaTime).f();
        if !self.ion_bolt_do_hit_check(b, num) {
            let (position, rotation) = self.with_component_instance(id, |_, inst, o| (inst.position(o), inst.rotation(o)));
            let right = mathf::rotate(rotation, [1.0, 0.0, 0.0]);
            let to = [position[0] + right[0] * num, position[1] + right[1] * num, position[2] + right[2] * num];
            self.with_component_instance(id, |_, inst, o| inst.set_world_position(o, to));
        }
    }

    /// IonBoltBehaviour.DoHitCheck(distance): true when the step hit something (the bolt is then at the hit point).
    pub(crate) fn ion_bolt_do_hit_check(&mut self, b: usize, distance: f32) -> bool {
        let id = self.ion_bolts[b].id;
        let (position, rotation) = self.with_component_instance(id, |_, inst, o| (inst.position(o), inst.rotation(o)));
        let right = mathf::rotate(rotation, [1.0, 0.0, 0.0]);
        let mask = self.ion_bolts[b].Layers;
        // Physics2D.Raycast(position, right, distance, Layers)
        let Some(hit) = self.raycast(V::new(position[0], position[1]), [right[0], right[1]], distance, mask, f32::NEG_INFINITY, f32::INFINITY) else {
            return false;
        };
        let (point, normal) = ([hit.point.x, hit.point.y], [hit.normal.x, hit.normal.y]);
        // Vector2.Reflect(right, normal)
        let dir = [right[0], right[1]];
        let num = (d(-2.0) * (d(normal[0]) * dir[0] + d(normal[1]) * dir[1]).f()).f();
        let reflected = [(d(num) * normal[0] + dir[0]).f(), (d(num) * normal[1] + dir[1]).f()];
        // Instantiate(ImpactEffect, hit.point, Euler(0, 0, z)).transform.up = normal: the effect has no physics and
        // nothing reads its pose before the up is set.
        let at = [point[0], point[1], 0.0];
        let effect = self.spawn_prefab(self.ion_bolt_impact_effect(b), at);
        self.tr_set_up(Xf::Object(effect), [normal[0], normal[1], 0.0]);
        let strength = self.ion_bolts[b].ImpactStrength;
        // ExplosionCreator.CreatePulseExplosion(hit.point, ImpactStrength, 1f, soundAndEffects: false): breakObjects defaults to true.
        self.create_pulse_explosion_with(at, strength, 1.0, false, true);
        // Physics2D.OverlapCircleNonAlloc(hit.point, AoERadius, buffer[16], Layers)
        let radius = self.ion_bolts[b].AoERadius;
        let hits = self.overlap_circle(V::new(point[0], point[1]), radius, mask);
        // -normal * ImpactStrength and -1f * ImpactStrength * hit.normal
        let push = [(d(0.0 - normal[0]) * strength).f(), (d(0.0 - normal[1]) * strength).f()];
        let k = (d(-1.0f32) * strength).f();
        for (_, collider) in hits.into_iter().take(BUFFER) {
            // PhysicalObjectsInWorldByTransform.TryGetValue(collider.transform): the collider sits on a physical's own
            // object, and that physical is in the world.
            let Some(p) = self.physical_of_collider(collider) else { continue };
            if !self.physicals[p].in_world || !self.collider_on_physical_object(collider, p) {
                continue;
            }
            if (self.random.value() as f64) > 0.5 {
                self.send_break(collider, Some(p), push);
            }
            let rb = self.physicals[p].rigidbody.clone();
            rb.add_impulse_at_position(&mut self.world, [k * normal[0], k * normal[1]], point);
            let bp = self.physicals[p].BurnProgress;
            self.physicals[p].BurnProgress = (d(bp) + d(0.5f32) * self.random.value()).f();
        }
        // hit.transform.SendMessage("Decal", new DecalInstruction(ImpactDecal, hit.point, Color.green, DecalSize))
        if let Some(ty) = self.ion_bolts[b].ImpactDecal {
            let size = self.ion_bolts[b].DecalSize;
            match self.hit_transform_physical(&hit).or_else(|| self.physical_of_collider(hit.collider)) {
                Some(p) => self.physical_decal(p, ty, point, size, [0.0, 1.0, 0.0, 1.0]),
                None if self.bare_colliders[&hit.collider].receives(&["DecalControllerBehaviour", "DecalRenderer", "DecalRendererController"]) => {
                    unimplemented!("mod-only: SendMessage Decal to the scripts of a collider without a PhysicalBehaviour")
                }
                None => {}
            }
        }
        // base.transform.right = vector
        self.tr_set_right(Xf::Object(id), [reflected[0], reflected[1], 0.0]);
        // DeleteAfterTime.Life *= 0.9f
        let dat = self.ion_bolts[b].delete_after;
        self.delete_afters[dat].Life = (d(self.delete_afters[dat].Life) * d(0.9f32)).f();
        // base.transform.position = hit.point
        self.with_component_instance(id, |_, inst, o| inst.set_world_position(o, [point[0], point[1], 0.0]));
        true
    }

    /// IonBoltBehaviour.Awake: DeleteAfterTime.OnEndOfLife.AddListener(the lambda below).
    pub fn ion_bolt_awake(&mut self, b: usize) {
        let d = self.ion_bolts[b].delete_after;
        self.delete_afters[d].ion_bolt = Some(b);
        self.listen_add(self.ion_bolts[b].id, super::continuous::Ev::OnEndOfLife);
    }

    /// IonBoltBehaviour.Awake's OnEndOfLife listener: Instantiate(EndOfLifeEffect, transform.position, identity),
    /// Particles.transform.SetParent(null) and Particles.AddComponent<DeleteAfterTime>().Life = 1f.
    pub(crate) fn ion_bolt_end_of_life(&mut self, b: usize) {
        let (id, particles) = (self.ion_bolts[b].id, self.ion_bolts[b].Particles);
        let at = self.with_component_instance(id, |_, inst, o| inst.position(o));
        let effect = self.ion_bolts[b].EndOfLifeEffect.expect("IonBoltBehaviour: Instantiate(null EndOfLifeEffect) (ArgumentException)");
        self.spawn_prefab(effect, at);
        self.set_parent_null_of(particles, |_, _, _| {});
        let (active, key) = self.with_component_instance(particles, |_, inst, o| (inst.objects[o].self_active, inst.objects[o].components[0]));
        if active {
            self.add_delete_after(key, 1.0);
        } else {
            // On an inactive object the component is added but never runs.
            self.allocate_instance_id();
        }
    }
}
