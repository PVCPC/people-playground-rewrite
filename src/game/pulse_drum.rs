//! PulseDrumBehaviour: a fan of rays that pushes what it hits by its own mass, with recoil and a one-shot sound.
#![allow(non_snake_case)]

use super::{d, mathf, script, Game, V};

/// PulseDrumBehaviour.buffer's length (its static setup: new RaycastHit2D[8]): RaycastNonAlloc's cap in Shoot.
pub(crate) const PULSE_DRUM_BUFFER: usize = 8;

script! {
    PulseDrum {
        barrelPosition: [f32; 2] = [0.0, 0.0],
        barrelDirection: [f32; 2] = [0.0, 0.0],
        BaseForce: f32 = 10.0,
        ChargeInfluence: f32 = 10.0,
        RayCount: i32 = 6,
        SpreadAngle: f32 = 30.0,
        Range: f32 = 2.0,
        EmitterSize: f32 = 0.5,
    }
    links {
        /// The component's instance id (its Source and Clips fields).
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// PhysicalBehaviour
        physical: usize = 0,
        /// Layers.m_Bits
        Layers: u32 = 0,
        /// Clips.Length (PickRandom draws).
        Clips: usize = 0,
    }
}

impl Game {
    /// PulseDrumBehaviour.Use
    pub fn pulse_drum_use(&mut self, i: usize) {
        if self.pulse_drums[i].enabled {
            self.pulse_drum_shoot(i);
        }
    }

    /// PulseDrumBehaviour.get_BarrelPosition: TransformPoint(barrelPosition) as a Vector2.
    pub(crate) fn pulse_drum_barrel_position(&self, i: usize) -> [f32; 2] {
        let s = &self.pulse_drums[i];
        let b = self.transform_point3(s.physical, [s.barrelPosition[0], s.barrelPosition[1], 0.0]);
        [b[0], b[1]]
    }

    /// PulseDrumBehaviour.get_BarrelDirection: TransformDirection(barrelDirection) * transform.localScale.x.
    pub(crate) fn pulse_drum_barrel_direction(&self, i: usize) -> [f32; 2] {
        let s = &self.pulse_drums[i];
        let (p, direction) = (s.physical, s.barrelDirection);
        let v = self.transform_direction(p, [direction[0], direction[1], 0.0]);
        let scale = self.physicals[p].transform.localScale[0];
        [v[0] * scale, v[1] * scale]
    }

    /// PulseDrumBehaviour.Shoot
    pub(crate) fn pulse_drum_shoot(&mut self, i: usize) {
        // Source.PlayOneShot(Clips.PickRandom())
        self.audio_one_shot_pick(self.pulse_drums[i].id, "Source", "Clips", self.pulse_drums[i].Clips);
        let s = &self.pulse_drums[i];
        let p = s.physical;
        let vector = self.pulse_drum_barrel_position(i);
        let vector2 = self.pulse_drum_barrel_direction(i);
        let s = &self.pulse_drums[i];
        let (spread, emitter, range, layers, count) = (s.SpreadAngle, s.EmitterSize, s.Range, s.Layers, s.RayCount);
        let num = (d(s.BaseForce) + d(self.physicals[p].Charge()) * s.ChargeInfluence).f();
        self.camera_shake((d(num) * 0.01f32).f(), vector);
        let k = 0.0 - num;
        let rb = self.physicals[p].rigidbody.clone();
        rb.add_force_at_position(&mut self.world, [k * vector2[0], k * vector2[1]], vector);
        let num2 = (d(spread) / 2.0f32).f();
        let num3 = (d(emitter) / 2.0f32).f();
        let num4 = count as f32 - 1.0;
        for ray in 0..count {
            let num5 = ray as f32 / num4;
            let z = (d(num5) * spread - num2).f();
            let num6 = (d(num5) * emitter - num3).f();
            // Quaternion.Euler(0f, 0f, z) * vector2: EulerToQuaternion (18073d0c0) with the CRT's sinf/cosf.
            let half = z * mathf::DEG2RAD * 0.5;
            let q = [0.0, 0.0, crate::unity::sinf(half), crate::unity::cosf(half)];
            let vector3 = mathf::rotate(q, [vector2[0], vector2[1], 0.0]);
            // Vector2.Perpendicular(vector2) * num6
            let origin = V::new(vector[0] - vector2[1] * num6, vector[1] + vector2[0] * num6);
            for hit in self.raycast_all(origin, [vector3[0], vector3[1]], range, layers, PULSE_DRUM_BUFFER) {
                // PhysicalObjectsInWorldByTransform.TryGetValue(hit.transform): the attached Rigidbody2D's transform.
                let Some(q) = self.physicals.iter().position(|x| !x.destroyed && x.in_world && x.rigidbody.body == hit.body) else { continue };
                let rb = self.physicals[q].rigidbody.clone();
                let f = (d(num) * rb.mass(&self.world)).f();
                rb.add_force_at_position(&mut self.world, [f * vector3[0], f * vector3[1]], [hit.point.x, hit.point.y]);
            }
        }
    }
}
