//! CustomFixedConstrainedBehaviour ([Obsolete], on no prefab or scene of the game): drags `Other` to a point and
//! angle fixed in this body's frame with Rigidbody2D.MovePosition and MoveRotation.
#![allow(non_snake_case)]

use super::mathf::delta_angle;
use super::{BodyOwner, Game};

#[derive(Clone, Debug)]
pub struct CustomFixedConstrained {
    pub id: i32,
    /// This object's physical (its Transform).
    pub physical: usize,
    pub Other: Option<BodyOwner>,
    pub LocalPosition: [f32; 2],
    pub RelativeAngle: f32,
    /// CustomFixedConstrainedBehaviour.constructor: AutoAnchor = true.
    pub AutoAnchor: bool,
    /// Awake's GetComponent<Rigidbody2D>().
    pub rb: Option<BodyOwner>,
}

impl Default for CustomFixedConstrained {
    fn default() -> Self {
        Self {
            id: Default::default(),
            physical: Default::default(),
            Other: Default::default(),
            LocalPosition: Default::default(),
            RelativeAngle: Default::default(),
            AutoAnchor: true,
            rb: Default::default(),
        }
    }
}

impl Game {
    fn custom_fixed_body(&mut self, owner: Option<BodyOwner>, what: &str) -> super::Rigidbody {
        let o = owner.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.sync_pending();
        self.owner_rigidbody(o).clone()
    }

    /// CustomFixedConstrainedBehaviour.Start: with AutoAnchor, the anchor is Other's body position in this
    /// Transform's space and the angle the DeltaAngle from Other's rotation to this body's.
    pub fn custom_fixed_start(&mut self, i: usize) {
        let s = self.custom_fixeds[i].clone();
        if !s.AutoAnchor {
            return;
        }
        let other = self.custom_fixed_body(s.Other, "Other");
        let p = self.world.body(other.body).xf.p;
        let local = self.inverse_transform_point(s.physical, [p.x, p.y, 0.0]);
        let other_rotation = other.rotation(&self.world);
        let rb = self.custom_fixed_body(s.rb, "rb");
        let angle = delta_angle(other_rotation, rb.rotation(&self.world));
        let s = &mut self.custom_fixeds[i];
        (s.LocalPosition, s.RelativeAngle) = ([local[0], local[1]], angle);
    }

    /// CustomFixedConstrainedBehaviour.FixedUpdate
    pub fn custom_fixed_fixed_update(&mut self, i: usize) {
        let s = self.custom_fixeds[i].clone();
        let rb = self.custom_fixed_body(s.rb, "rb");
        let point = self.rigidbody_point_relative(&rb, s.LocalPosition);
        let angle = super::physical::utils_mod(rb.rotation(&self.world) + s.RelativeAngle, 360.0);
        let other = s.Other.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.rigidbody_move_position(other, point);
        self.rigidbody_move_rotation(other, angle);
    }
}
