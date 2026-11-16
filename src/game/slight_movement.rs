//! SlightMovement: a transform wandering on Perlin noise.
#![allow(non_snake_case)]

use super::{mathf, Game};

#[derive(Clone, Debug)]
pub struct SlightMovement {
    pub id: i32,
    pub Intensity: f32,
    pub Speed: f32,
}

impl Default for SlightMovement {
    fn default() -> Self {
        Self {
            id: Default::default(),
            Intensity: 1.0,
            Speed: 1.0,
        }
    }
}

impl SlightMovement {
    /// SlightMovement.constructor: Intensity = 1f, Speed = 1f.
    pub fn new(id: i32) -> Self {
        SlightMovement { id, Intensity: 1.0, Speed: 1.0 }
    }
}

/// SlightMovement.Update's position for time `t`: `Intensity * Utils.GetPerlin2Mapped(num, num + 7892.387f)`, z 0 (Vector2 to Vector3).
fn slight_position(s: &SlightMovement, t: f32) -> [f32; 3] {
    let num = s.Speed * t;
    let p = mathf::perlin2_mapped(num, num + 7892.387f32);
    [s.Intensity * p[0], s.Intensity * p[1], 0.0]
}

impl Game {
    /// SlightMovement.Update
    pub fn slight_movement_update(&mut self, i: usize) {
        let s = &self.slight_movements[i];
        let (id, pos) = (s.id, slight_position(s, self.time.time));
        self.with_component_instance(id, |_, inst, o| inst.set_world_position(o, pos));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slight_position_scales_with_intensity() {
        let mut s = SlightMovement::new(1);
        let p = slight_position(&s, 3.5);
        s.Intensity = 2.0;
        let q = slight_position(&s, 3.5);
        assert_eq!((q[0], q[1], q[2]), (2.0 * p[0], 2.0 * p[1], 0.0));
    }
}
