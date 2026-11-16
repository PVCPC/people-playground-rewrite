//! ObstructionEmitterBehaviour. Use toggles Activated; an active emitter casts a 0.05 circle along transform.up (up to
//! MaximumDistance, against HitLayers) every FixedUpdate and gives its EdgeCollider2D the two points of that beam, so the
//! collider ends at what the cast hit. The AudioSource (Play, Stop) is
//! presentation; only the AudioSource.time draw of Random.value is kept, in order.
#![allow(non_snake_case)]

use super::{script, Game};

script! {
    ObstructionEmitter {
        Activated: bool = false,
        MaximumDistance: f32 = 10.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        enabled: bool = true,
        /// The PhysicalBehaviour on the same GameObject (transform.position and transform.up are its).
        physical: usize = 0,
        /// EdgeCollider: (object, component) in its instance.
        EdgeCollider: (usize, usize) = (0, 0),
        /// HitLayers (LayerMask bits).
        HitLayers: u32 = 0,
        /// LineRenderer: its index in `Game::lines`.
        line: usize = 0,
        /// lastDist (float.MaxValue until the first SetNewDistance).
        lastDist: f32 = f32::MAX,
        /// vertices[1] (vertices[0] stays (0, 0)).
        vertex1: [f32; 2] = [0.0, 0.0],
        /// LineRenderer or AudioSource is null (NullReferenceException where UpdateActivation, SetNewDistance and OnDisable use it).
        line_null: bool = false,
        audio_null: bool = false,
    }
}

impl Game {
    /// ObstructionEmitterBehaviour.Start
    pub fn obstruction_emitter_start(&mut self, i: usize) {
        self.obstruction_emitter_update_activation(i);
    }

    /// ObstructionEmitterBehaviour.OnEnable
    pub fn obstruction_emitter_on_enable(&mut self, i: usize) {
        self.obstruction_emitter_update_activation(i);
    }

    /// ObstructionEmitterBehaviour.Use
    pub fn obstruction_emitter_use(&mut self, i: usize) {
        if self.obstruction_emitters[i].enabled {
            self.obstruction_emitters[i].Activated = !self.obstruction_emitters[i].Activated;
            self.obstruction_emitter_update_activation(i);
        }
    }

    /// ObstructionEmitterBehaviour.FixedUpdate
    pub fn obstruction_emitter_fixed_update(&mut self, i: usize) {
        if self.obstruction_emitters[i].Activated {
            self.obstruction_emitter_calculate_shape(i);
        }
    }

    /// ObstructionEmitterBehaviour.OnDisable: EdgeCollider.enabled = false, LineRenderer.enabled = false.
    pub fn obstruction_emitter_on_disable(&mut self, i: usize, inst: &mut super::spawn::Instance) {
        let key = self.obstruction_emitters[i].EdgeCollider;
        self.collider_set_enabled(inst, key, false);
        if self.obstruction_emitters[i].line_null {
            crate::game::menu::npe();
        }
        let line = self.obstruction_emitters[i].line;
        self.lines[line].enabled = false;
    }

    /// ObstructionEmitterBehaviour.UpdateActivation: the shape first when Activated, then EdgeCollider.enabled = Activated; an activation also draws
    /// LineRenderer.enabled = Activated and AudioSource.time = clip.length * Random.value (the sound itself is presentation).
    pub(crate) fn obstruction_emitter_update_activation(&mut self, i: usize) {
        let activated = self.obstruction_emitters[i].Activated;
        if activated {
            self.obstruction_emitter_calculate_shape(i);
        }
        let (id, key) = (self.obstruction_emitters[i].id, self.obstruction_emitters[i].EdgeCollider);
        self.with_component_instance(id, |g, inst, _| g.collider_set_enabled(inst, key, activated));
        if self.obstruction_emitters[i].line_null {
            crate::game::menu::npe();
        }
        let line = self.obstruction_emitters[i].line;
        self.lines[line].enabled = activated;
        if self.obstruction_emitters[i].audio_null {
            crate::game::menu::npe();
        }
        if activated {
            self.random.value();
        }
    }

    /// ObstructionEmitterBehaviour.CalculateShape: Physics2D.CircleCast(position, 0.05f, up, MaximumDistance, HitLayers); the beam is as long as the hit, or MaximumDistance.
    pub(crate) fn obstruction_emitter_calculate_shape(&mut self, i: usize) {
        let s = &self.obstruction_emitters[i];
        let (p, maximum, mask) = (s.physical, s.MaximumDistance, s.HitLayers);
        let position = self.physicals[p].transform.position;
        let up = self.transform_up(p);
        let hit = self.circle_cast([position[0], position[1]], 0.05, [up[0], up[1]], maximum, mask);
        self.obstruction_emitter_set_new_distance(i, hit.map_or(maximum, |h| h.distance));
    }

    /// ObstructionEmitterBehaviour.SetNewDistance: a change of more than 0.01 moves the second point to (0, distance - 0.1) and gives the EdgeCollider both points; the LineRenderer's second point moves to (0, distance).
    pub(crate) fn obstruction_emitter_set_new_distance(&mut self, i: usize, distance: f32) {
        if ((distance - self.obstruction_emitters[i].lastDist).abs() as f64) <= 0.01 {
            return;
        }
        let s = &mut self.obstruction_emitters[i];
        s.lastDist = distance;
        s.vertex1 = [0.0, distance - 0.1];
        if s.line_null {
            crate::game::menu::npe();
        }
        let line = s.line;
        self.line_set_position(line, 1, [0.0, distance, 0.0]);
        let s = &mut self.obstruction_emitters[i];
        let (id, key, points) = (s.id, s.EdgeCollider, vec![[0.0, 0.0], s.vertex1]);
        self.with_component_instance(id, |g, inst, _| g.edge_collider_set_points(inst, key, points));
    }
}
