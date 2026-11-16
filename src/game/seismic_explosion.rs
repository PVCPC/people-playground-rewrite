//! SeismicChargeExplosionBehaviour (the Energy Vessel Explosive's blast) and EnergyVesselLightningBehaviour (its
//! bolts). Awake parents BoltCount bolts at its origin; Update grows the object (and its PointEffector2D's trigger)
//! along the scale curves. After MuteEndSecond the first FixedUpdate hides the bolts, turns the magnet off and throws
//! everything within 350 outward, with a 25 fragment hit and a random charge. Each bolt waits a random 0.1 to 1
//! second, picks a random direction and life, and at the first hit of its ray sets off a 16-ray explosion and a spark.
//! The camera shake and the sprite are presentation.
#![allow(non_snake_case)]

use super::destroyable::{AttachParent, AttachedClone, Step};
use super::explosion::{normalized3, ExplosionParameters};
use super::{d, mathf, script, Game};
use crate::assets::db::ObjRef;
use crate::b2::math::Vec2 as V;
use crate::game_assets::{Curve, CurveCache};

script! {
    SeismicExplosion {
        SizeMultiplier: f32 = 1.0,
        Duration: f32 = 1.0,
        Strength: f32 = 55.0,
        MuteStartSecond: f32 = 0.0,
        MuteEndSecond: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// transform.position (a root without a PhysicalBehaviour; nothing moves it).
        position: [f32; 3] = [0.0; 3],
        XScaleCurve: Option<Curve> = None,
        YScaleCurve: Option<Curve> = None,
        x_cache: CurveCache = CurveCache::default(),
        y_cache: CurveCache = CurveCache::default(),
        /// Magnet: the PointEffector2D component id.
        Magnet: i32 = 0,
        BoltCount: u32 = 32,
        bolt: Option<ObjRef> = None,
        /// A Collider2D of its GameObject, the bolts' parent.
        collider: i32 = 0,
        /// bolts: the clones' root Transform ids.
        bolts: Vec<i32> = Vec::new(),
        time: f32 = 0.0,
        /// muted
        muted: bool = false,
    }
}

script! {
    EnergyBolt {
        life: f32 = 0.0,
        seed: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        Spark: Option<ObjRef> = None,
        /// lineRenderer: its index in `Game::lines` (enabled is the line's).
        line: usize = 0,
        /// curve and its evaluation cache.
        curve: Option<Curve> = None,
        curve_cache: CurveCache = CurveCache::default(),
        t: f32 = 0.0,
        targetDirection: [f32; 2] = [0.0; 2],
        hasExploded: bool = false,
    }
}

impl Game {
    /// SeismicChargeExplosionBehaviour.Awake: BoltCount times Instantiate(bolt, transform.position, identity, transform)
    /// with localPosition zero. The object never moves, so each bolt keeps its spawn pose (`driven`); the parent
    /// carries SetActive and Destroy.
    pub(crate) fn seismic_explosion_awake(&mut self, mut db: Option<&mut crate::assets::db::AssetDb>, inst: &super::spawn::Instance, o: usize, i: usize) {
        let s = &self.seismic_explosions[i];
        let (count, collider, position) = (s.BoltCount, s.collider, s.position);
        let key = inst.objects[o].components[0];
        let q = inst.objects[o].trs.q;
        for _ in 0..count {
            let bolt = self.seismic_explosions[i].bolt.expect("SeismicChargeExplosionBehaviour: Instantiate(null bolt) (ArgumentException)");
            let id = self.spawn_prefab_with(db.as_deref_mut(), bolt, position);
            let s = self.with_component_instance(id, |_, clone, x| clone.objects[x].trs.s);
            // The local rotation under the parent: inverse(parent rotation) * identity.
            let local = crate::unity::transform::Trs { t: [0.0; 4], q: [-q[0], -q[1], -q[2], q[3]], s };
            self.attached_clones.entry(key).or_default().push(AttachedClone { id, parent: AttachParent::Bare(collider), local, driven: true });
            self.seismic_explosions[i].bolts.push(id);
        }
    }

    /// localScale = new Vector3(XScaleCurve.Evaluate(x), YScaleCurve.Evaluate(x), 1f) * SizeMultiplier
    fn seismic_explosion_scale(&mut self, i: usize, x: f32) {
        let s = &mut self.seismic_explosions[i];
        let (cx, cy) = (s.XScaleCurve.clone().expect("XScaleCurve"), s.YScaleCurve.clone().expect("YScaleCurve"));
        let k = s.SizeMultiplier;
        let scale = [cx.evaluate(&mut s.x_cache, x) * k, cy.evaluate(&mut s.y_cache, x) * k, 1.0 * k];
        let id = s.id;
        self.with_component_instance(id, |_, inst, o| inst.set_local_scale(o, scale));
    }

    /// SeismicChargeExplosionBehaviour.Start: the scale at time 0, then ExplosionAudioSource.clip = SeismicChargeSound and Play.
    pub fn seismic_explosion_start(&mut self, i: usize) {
        let t = self.seismic_explosions[i].time;
        self.seismic_explosion_scale(i, t);
        let id = self.seismic_explosions[i].id;
        let src = self.audio_src(id, "ExplosionAudioSource");
        let clip = self.audio.clip(id, "SeismicChargeSound");
        self.audio.set_clip(src, clip);
        self.audio.play(src);
    }

    /// SeismicChargeExplosionBehaviour.Update.
    pub fn seismic_explosion_update(&mut self, i: usize) {
        let dt = self.time.deltaTime;
        let s = &mut self.seismic_explosions[i];
        s.time += dt;
        let x = s.time / s.Duration;
        self.seismic_explosion_scale(i, x);
        let s = &self.seismic_explosions[i];
        let (time, start, end, muted) = (s.time, s.MuteStartSecond, s.MuteEndSecond, s.muted);
        if time > start && time <= end && !muted {
            self.seismic_explosions[i].muted = true;
            self.camera_listener_mute(true);
        }
        if time > end && self.seismic_explosions[i].muted {
            self.seismic_explosions[i].muted = false;
            self.camera_listener_mute(false);
        }
        let s = &self.seismic_explosions[i];
        if s.time < s.MuteStartSecond {
            let at = s.position;
            self.camera_shake_at(20.0, [at[0], at[1]], 0.5);
        }
    }

    /// SeismicChargeExplosionBehaviour.FixedUpdate
    pub fn seismic_explosion_fixed_update(&mut self, i: usize) {
        let s = &self.seismic_explosions[i];
        let magnet = s.Magnet;
        if !(s.time > s.MuteEndSecond) || !self.effector_enabled(magnet) {
            return;
        }
        for bolt in s.bolts.clone() {
            self.with_component_instance(bolt, |g, inst, o| g.set_active_scripts(inst, o, false));
        }
        let (position, strength) = (self.seismic_explosions[i].position, self.seismic_explosions[i].Strength);
        self.camera_shake(25.0, [position[0], position[1]]);
        self.effector_set_enabled(magnet, false);
        let mask = self.effector_collider_mask(magnet) as u32;
        for (_, collider) in self.overlap_circle(V::new(position[0], position[1]), 350.0, mask) {
            // SendMessage("OnFragmentHit", 25): the engine converts the boxed int for the float parameter
            // (MonoBehaviour::InvokeMethodOrCoroutineChecked, 1808b7370).
            let body = self.world.collider_body(collider);
            self.send_on_fragment_hit(collider, 25.0);
            // A collider its OnFragmentHit destroyed (Break) has no shapes left: Collider2D.GetAttachedRigidbody then
            // finds the Rigidbody2D through the GameObject hierarchy (180c04c60), which is the body it had.
            if let Some(rb) = body.and_then(|body| self.rigidbody_of_body(body)) {
                let at = self.collider_position(collider);
                let n = normalized3([position[0] - at[0], position[1] - at[1], position[2] - at[2]]);
                let k = rb.mass(&self.world) * (0.0 - strength);
                rb.add_force_impulse(&mut self.world, [k * n[0], k * n[1]]);
            }
            // collider2D.GetComponent<PhysicalBehaviour>()
            if let Some(p) = self.physical_of_collider(collider).filter(|&p| self.collider_on_physical_object(collider, p)) {
                self.physicals[p].charge = 150.0 * self.random.value();
            }
        }
    }

    /// EnergyVesselLightningBehaviour.Update
    pub fn energy_bolt_update(&mut self, b: usize) {
        let line = self.energy_bolts[b].line;
        if !self.lines[line].enabled {
            return;
        }
        self.energy_bolts[b].t += self.time.smoothDeltaTime;
        if self.energy_bolts[b].t > self.energy_bolts[b].life {
            self.energy_bolts[b].t = 0.0;
            self.energy_bolts[b].hasExploded = false;
            // StartCoroutine(Init()): lineRenderer.enabled = false, up to its WaitWaitForSeconds.
            self.lines[line].enabled = false;
            let wait = self.random.range(0.1, 1.0);
            self.start_wait(self.energy_bolts[b].id, wait, Step::EnergyBoltInit(b));
        }
        let num = self.energy_bolts[b].t;
        let id = self.energy_bolts[b].id;
        let position = self.with_component_instance(id, |_, inst, o| inst.position(o));
        let direction = self.energy_bolts[b].targetDirection;
        let mut vector = [0.0f32; 3];
        match self.raycast_first(V::new(position[0], position[1]), direction, f32::INFINITY) {
            Some(hit) => {
                vector = [hit.point.x, hit.point.y, 0.0];
                if !self.energy_bolts[b].hasExploded {
                    self.explosion_creator_explode(ExplosionParameters::new_big(16, vector, 15.0, 5.0, false, true, 0.0, 0));
                    let spark = self.energy_bolts[b].Spark.expect("EnergyVesselLightningBehaviour: Instantiate(null Spark) (ArgumentException)");
                    self.spawn_prefab(spark, vector);
                    self.energy_bolts[b].hasExploded = true;
                }
            }
            None => self.energy_bolts[b].t = self.energy_bolts[b].life,
        }
        // lineRenderer.positionCount = 3 * Mathf.Clamp((int)Vector3.Distance(position, vector) * 3, 5, 64)
        let (dx, dy, dz) = (position[0] - vector[0], position[1] - vector[1], position[2] - vector[2]);
        let distance = (d(dx) * dx + d(dy) * dy + d(dz) * dz).0.sqrt() as f32;
        let count = 3 * ((distance as i32).wrapping_mul(3)).clamp(5, 64);
        self.line_set_position_count(line, count as usize);
        let seed = self.energy_bolts[b].seed;
        let mut array = Vec::with_capacity(count as usize);
        for i in 0..count {
            let time = i as f32 / (count - 1) as f32;
            let v2 = [mathf::lerp(position[0], vector[0], time), mathf::lerp(position[1], vector[1], time), mathf::lerp(position[2], vector[2], time)];
            let perlin = mathf::perlin;
            let v3 = [
                (d(perlin((d(v2[0]) * 0.2f32 - seed).f(), (d(v2[1]) * 0.2f32 + seed).f())) * 2.0f32 - 1.0f32).f(),
                perlin((d(v2[0]) * 0.2f32 + seed).f(), (d(v2[1]) * 0.2f32 - seed).f()),
            ];
            let s = &mut self.energy_bolts[b];
            let w = s.curve.as_ref().expect("EnergyVesselLightningBehaviour.curve").evaluate(&mut s.curve_cache, time);
            let n4 = [
                (d(perlin((d(v2[0]) * 0.03f32 - seed).f(), (d(v2[1]) * 0.03f32 + seed).f())) * 2.0f32 - 1.0f32).f(),
                (d(perlin((d(v2[0]) * 0.03f32 + seed).f(), (d(v2[1]) * 0.03f32 - seed).f())) * 2.0f32 - 1.0f32).f(),
            ];
            let v4 = [w * n4[0], w * n4[1]];
            let k = 36.25f32 * num;
            let v5 = [k * v3[0] + v4[0] * 21.3, k * v3[1] + v4[1] * 21.3, k * 0.0 + 0.0 * 21.3];
            let sr = energy_bolt_smooth_random(12.0, self.time.time, seed, [v2[0], v2[1]]);
            array.push([(v2[0] + v5[0]) + sr[0] * 2.0, (v2[1] + v5[1]) + sr[1] * 2.0, (v2[2] + v5[2]) + 0.0 * 2.0]);
        }
        self.line_set_positions(line, &array);
    }

    /// EnergyVesselLightningBehaviour.Init after its WaitForSeconds.
    pub(crate) fn energy_bolt_init(&mut self, b: usize) {
        let line = self.energy_bolts[b].line;
        self.lines[line].enabled = true;
        self.energy_bolts[b].seed = self.random.range(-9000.0, 9000.0);
        let d = self.random.on_unit_sphere();
        self.energy_bolts[b].targetDirection = [d[0], d[1]];
        self.energy_bolts[b].life = if (self.random.value() as f64) > 0.95 { self.random.range(0.1, 0.3) } else { self.random.range(0.02, 0.08) };
        let width = self.random.range(1.0, 2.0);
        self.lines[line].width = width;
    }
}

impl Game {
    /// EnergyVesselLightningBehaviour.Awake: lineRenderer = GetComponent<LineRenderer>() (`line`, whose enabled starts as the prefab's m_Enabled),
    /// useWorldSpace = true and sharedMaterial = LightningMaterial.
    pub(super) fn energy_bolt_awake(&mut self, line: usize, lightning_material: Option<ObjRef>) {
        self.lines[line].use_world_space = true;
        self.lines[line].material = lightning_material;
    }
}

/// EnergyVesselLightningBehaviour.SmoothRandom(frq, pos): num = frq * Time.time, then
/// (PerlinNoise(num, seed + pos.x) * 2 - 1, PerlinNoise(seed, num + pos.y) * 2 - 1).
pub(crate) fn energy_bolt_smooth_random(frq: f32, time: f32, seed: f32, pos: [f32; 2]) -> [f32; 2] {
    let num = frq * time;
    let perlin = mathf::perlin;
    [(d(perlin(num, (d(seed) + pos[0]).f())) * 2.0f32 - 1.0f32).f(), (d(perlin(seed, (d(num) + pos[1]).f())) * 2.0f32 - 1.0f32).f()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_bolt_smooth_random_is_two_perlin_samples_mapped_to_minus_one_one() {
        let (frq, time, seed, pos) = (12.0f32, 3.25f32, 1234.5f32, [4.0f32, -2.5f32]);
        let r = energy_bolt_smooth_random(frq, time, seed, pos);
        let num = frq * time;
        assert_eq!(r[0], mathf::perlin(num, seed + pos[0]) * 2.0 - 1.0);
        assert_eq!(r[1], mathf::perlin(seed, num + pos[1]) * 2.0 - 1.0);
        assert!(r.iter().all(|v| (-1.0..=1.0).contains(v)));
    }
}
