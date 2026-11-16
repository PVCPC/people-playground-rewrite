//! ReactorA5ArcBehaviour: Flash (the bolt's noise draw, the capsule overlap along the electrodes and Affect on each hit;
//! the line's fade is rendering) and GenerateSmallArc (a short lightning from this electrode toward the other).
#![allow(non_snake_case)]

use super::{mathf, script, Game};
use crate::b2::math::Vec2 as V;
use crate::b2::shape::Capsule;
use crate::b2::world::Shape;

script! {
    ReactorA5Arc {
        OverlapCapsuleRadius: f32 = 3.0,
        HitTemperatureAdjustment: f32 = 300.0,
        HitChargeAdjustment: f32 = 1000.0,
        HitForce: f32 = 15.0,
        NoiseFrequency: f32 = 2.95,
        NoiseAmplitude: f32 = 2.0,
        FlashDuration: f32 = 0.15,
        Color: [f32; 4] = [0.0; 4],
    }
    links {
        id: i32 = 0,
        enabled: bool = true,
        /// OtherElectrode's object in the prefab.
        electrode: usize = 0,
        /// Exclude: the Collider2D instance ids.
        exclude: Vec<i32> = Vec::new(),
        /// OverlapMask.
        mask: u32 = 0,
        /// LineRenderer, in `Game::lines`.
        line: usize = 0,
        /// Points.Length
        points: usize = 64,
        /// The running FlashRoutines' t, in start order.
        flashes: Vec<f32> = Vec::new(),
    }
}

impl Game {
    /// The arc's own and the other electrode's world positions.
    fn arc_positions(&mut self, a: usize) -> ([f32; 3], [f32; 3]) {
        let (id, electrode) = (self.reactor_a5_arcs[a].id, self.reactor_a5_arcs[a].electrode);
        self.with_component_instance(id, |_, inst, o| (inst.position(o), inst.position(electrode)))
    }

    /// ReactorA5ArcBehaviour.Flash, ReactorA5ArcBehaviour.GenerateBolt and ReactorA5ArcBehaviour.FlashRoutine: GenerateBolt's Random.Range(-100, 100) (the Perlin points are the line's), then
    /// FlashRoutine's first segment: OverlapCapsuleNonAlloc(buffer of 32) and Affect on each; the fade is rendering.
    pub fn reactor_a5_arc_flash(&mut self, id: i32) {
        let a = self.reactor_a5_arcs.iter().position(|x| x.id == id).expect("a ReactorA5ArcBehaviour in the arena");
        self.reactor_a5_arc_generate_bolt(a);
        self.reactor_a5_arc_flash_routine(a);
    }

    /// ReactorA5ArcBehaviour.GenerateBolt
    pub(crate) fn reactor_a5_arc_generate_bolt(&mut self, a: usize) {
        let (position, position2) = self.arc_positions(a);
        // GenerateBolt: Points[i] = Lerp(position, position2, i / (n - 1)), the inner ones moved by two Perlin octaves.
        let x = self.random.range(-100.0, 100.0);
        let (n, freq, amp) = (self.reactor_a5_arcs[a].points, self.reactor_a5_arcs[a].NoiseFrequency, self.reactor_a5_arcs[a].NoiseAmplitude);
        let points: Vec<[f32; 3]> = (0..n)
            .map(|i| {
                let t = (super::d(i as f32) / (n as f32 - 1.0)).f().clamp(0.0, 1.0);
                // Vector3.Lerp: a + (b - a) * Clamp01(t).
                let mut p = [0, 1, 2].map(|c| (super::d(position[c]) + (super::d(position2[c]) - position[c]) * t).f());
                if i != 0 && i != n - 1 {
                    let a1 = mathf::perlin2_mapped(x, (super::d(i as f32) * freq).f());
                    let a2 = mathf::perlin2_mapped(x, (super::d(i as f32) * freq * 8.0).f());
                    let w = (super::d(0.2) * amp).f();
                    for c in 0..2 {
                        p[c] = (super::d(p[c]) + (super::d(a1[c]) * amp).f()).f();
                        p[c] = (super::d(p[c]) + (super::d(w) * a2[c]).f()).f();
                    }
                }
                p
            })
            .collect();
        // LineRenderer.SetPositions(Points) is the line's: the arc draws the points it made.
        let line = self.reactor_a5_arcs[a].line;
        self.lines[line].positions = points;
    }

    /// ReactorA5ArcBehaviour.FlashRoutine, its first MoveNext (it then yields null; the rest is reactor_a5_arc_coroutines).
    pub(crate) fn reactor_a5_arc_flash_routine(&mut self, a: usize) {
        let (position, position2) = self.arc_positions(a);
        let vector = [position[0] - position2[0], position[1] - position2[1], position[2] - position2[2]];
        let magnitude = (super::d(vector[0]) * vector[0] + super::d(vector[1]) * vector[1] + super::d(vector[2]) * vector[2]).0.sqrt() as f32;
        let normalized = if magnitude > 1e-5 { [vector[0] / magnitude, vector[1] / magnitude] } else { [0.0, 0.0] };
        let center = [(position[0] + position2[0]) * 0.5, (position[1] + position2[1]) * 0.5];
        let size = [magnitude, self.reactor_a5_arcs[a].OverlapCapsuleRadius * 2.0];
        let angle = crate::unity::atan2f(normalized[1], normalized[0]) * 57.29578;
        let hits = self.overlap_capsule_horizontal(center, size, angle, self.reactor_a5_arcs[a].mask);
        for (_, c) in hits.into_iter().take(REACTOR_ARC_BUFFER) {
            self.reactor_a5_arc_affect(a, c, center);
        }
        // LineRenderer.enabled = true, then the loop's first pass before its first yield.
        let line = self.reactor_a5_arcs[a].line;
        self.lines[line].enabled = true;
        self.reactor_a5_arcs[a].flashes.push(0.0);
        let k = self.reactor_a5_arcs[a].flashes.len() - 1;
        self.reactor_a5_arc_fade_step(a, k);
    }

    /// Physics2D.OverlapCapsuleAll, CapsuleDirection2D.Horizontal (OverlapCapsuleQuery2D, 180be5fc0 and 180bf1080): the
    /// size clamped to 1e-4, the angle in radians, the capsule (b2 set-as-capsule, 180bb5af0) of radius size.y / 2
    /// whose end points lie max(size.x - size.y, 1e-4) / 2 either way along the angle from the centre.
    fn overlap_capsule_horizontal(&mut self, center: [f32; 2], size: [f32; 2], angle: f32, mask: u32) -> Vec<(Option<usize>, i32)> {
        let min = f32::from_bits(0x38d1b717);
        let sx = if size[0] > min { size[0] } else { min };
        let sy = if size[1] > min { size[1] } else { min };
        let length = if sx - sy > min { sx - sy } else { min };
        let rad = angle * 0.017453292;
        let (sin, cos) = (crate::unity::sinf(rad), crate::unity::cosf(rad));
        let h = length * 0.5;
        let v1 = V::new(cos * -h - sin * 0.0 + center[0], sin * -h + cos * 0.0 + center[1]);
        let v2 = V::new(cos * h - sin * 0.0 + center[0], sin * h + cos * 0.0 + center[1]);
        self.sync_pending();
        let shape = Shape::Capsule(Capsule { radius: sy * 0.5, vertices: [v1, v2] });
        let identity = crate::b2::math::Transform::default_identity();
        let aabb = shape.compute_aabb(&identity, 0);
        self.overlap_query(self.world.query_aabb(&aabb), mask, f32::NEG_INFINITY, f32::INFINITY, |other, child, xf| shape.test_overlap(0, &identity, other, child, xf))
    }

    /// ReactorA5ArcBehaviour.Start: LineRenderer.enabled = false, positionCount = Points.Length (link.rs sets them as the arc
    /// links: nothing draws or reads the line before its first frame).
    pub(crate) fn reactor_a5_arc_start(&mut self, a: usize) {
        let (line, n) = (self.reactor_a5_arcs[a].line, self.reactor_a5_arcs[a].points);
        self.lines[line].enabled = false;
        self.lines[line].positions.resize(n, [0.0; 3]);
    }

    /// FlashRoutine after its first yield, each frame with the coroutines that wait on null (after Update): while t <
    /// FlashDuration, t += deltaTime and both line colours Color * (1 - t / FlashDuration); then LineRenderer.enabled = false.
    /// A destroyed arc's routines stop.
    pub fn reactor_a5_arc_coroutines(&mut self) {
        for a in 0..self.reactor_a5_arcs.len() {
            if self.reactor_a5_arcs[a].flashes.is_empty() {
                continue;
            }
            if self.destroyed_components.contains(&self.reactor_a5_arcs[a].id) {
                self.reactor_a5_arcs[a].flashes.clear();
                continue;
            }
            let mut k = 0;
            while k < self.reactor_a5_arcs[a].flashes.len() {
                if self.reactor_a5_arcs[a].flashes[k] < self.reactor_a5_arcs[a].FlashDuration {
                    self.reactor_a5_arc_fade_step(a, k);
                    k += 1;
                } else {
                    let line = self.reactor_a5_arcs[a].line;
                    self.lines[line].enabled = false;
                    self.reactor_a5_arcs[a].flashes.remove(k);
                }
            }
        }
    }

    /// One pass of FlashRoutine's loop body.
    fn reactor_a5_arc_fade_step(&mut self, a: usize, k: usize) {
        let x = &mut self.reactor_a5_arcs[a];
        x.flashes[k] = (super::d(x.flashes[k]) + self.time.deltaTime).f();
        let k = (super::d(1.0) - super::d(x.flashes[k]) / x.FlashDuration).f();
        let color = x.Color.map(|c| (super::d(c) * k).f());
        let line = x.line;
        self.lines[line].gradient.set_start_color(color);
        self.lines[line].gradient.set_end_color(color);
    }
    /// ReactorA5ArcBehaviour.Affect
    pub(crate) fn reactor_a5_arc_affect(&mut self, a: usize, c: i32, center: [f32; 2]) {
        if !self.collider_alive(c) || self.reactor_a5_arcs[a].exclude.contains(&c) {
            return;
        }
        let Some(p) = self.physical_of_collider(c).filter(|&p| self.physicals[p].in_world) else { return };
        let (temperature, charge, force) = (self.reactor_a5_arcs[a].HitTemperatureAdjustment, self.reactor_a5_arcs[a].HitChargeAdjustment, self.reactor_a5_arcs[a].HitForce);
        let ph = &mut self.physicals[p];
        if ph.SimulateTemperature {
            ph.Temperature += temperature;
        }
        if ph.ConductOverride && !ph.ForceNoCharge {
            ph.charge = ph.Charge() + charge;
        }
        if self.physicals[p].rigidbody.bodyType == 0 {
            let position = self.physicals[p].transform.position;
            let n = mathf::normalized2([position[0] - center[0], position[1] - center[1]]);
            let r = self.random.inside_unit_circle();
            let n = [n[0] + r[0] * 0.2, n[1] + r[1] * 0.2];
            let rb = self.physicals[p].rigidbody.clone();
            rb.add_force(&mut self.world, [force * n[0], force * n[1]]);
        }
        if self.random.value() > 0.9 {
            self.send_slice(p);
        }
        if self.random.value() > 0.5 {
            let v = self.random.inside_unit_circle();
            self.send_break(c, Some(p), v);
        }
        if self.random.value() > 0.5 {
            let amount = self.random.value() * force;
            self.send_damage(p, amount);
        }
    }

    /// ReactorA5ArcBehaviour.GenerateSmallArc: SimpleLightningEmitterBehaviour.Emit from this electrode toward the other
    /// (range 5, attraction 3, 32 iterations, step 0.5, randomness 0.5).
    pub fn reactor_a5_arc_generate_small_arc(&mut self, id: i32) {
        let a = self.reactor_a5_arcs.iter().position(|x| x.id == id).expect("a ReactorA5ArcBehaviour in the arena");
        let (position, position2) = self.arc_positions(a);
        let v = [position2[0] - position[0], position2[1] - position[1], position2[2] - position[2]];
        let m = (super::d(v[0]) * v[0] + super::d(v[1]) * v[1] + super::d(v[2]) * v[2]).0.sqrt() as f32;
        let n = if m > 1e-5 { [v[0] / m, v[1] / m] } else { [0.0, 0.0] };
        let mask = self.reactor_a5_arcs[a].mask;
        self.lightning_emit([position[0], position[1]], n, 5.0, 3.0, 32, 0.5, 0.5, mask, super::lightning::LightningHit::ReactorArc(a));
    }

    /// ReactorA5ArcBehaviour.OnSmallArcHit
    pub(crate) fn reactor_a5_arc_small_hit(&mut self, a: usize, p: usize, point: [f32; 2]) {
        if point[0].is_nan() || point[1].is_nan() {
            return;
        }
        let (charge, temperature) = (self.reactor_a5_arcs[a].HitChargeAdjustment / 5.0, self.reactor_a5_arcs[a].HitTemperatureAdjustment / 5.0);
        let ph = &mut self.physicals[p];
        ph.charge = ph.Charge() + charge;
        ph.Temperature += temperature;
    }
}

/// ReactorA5ArcBehaviour.buffer: static Collider2D[32] (its static constructor; the rewrite's code uses it as the cap).
pub(crate) const REACTOR_ARC_BUFFER: usize = 32;
