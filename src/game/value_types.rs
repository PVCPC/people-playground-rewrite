//! The game's small value types: DamagePoint, RotatedRectangle, LineSegment and ComponentTagContainer.TaggedComponent
//! (Equals, operators and the hand-written GetHashCode with Mono's float, Vector2 and string hashes), and Utils.Rotate.
#![allow(non_snake_case)]

use super::{d, mathf};
use crate::unity::Random;

use super::decal_renderer::float_hash as hash_f32;

/// Vector2.GetHashCode: x.GetHashCode() ^ (y.GetHashCode() << 2)
fn hash_v2(v: [f32; 2]) -> i32 {
    hash_f32(v[0]) ^ (hash_f32(v[1]) << 2)
}



/// Vector2.Equals(Vector2): x == other.x && y == other.y (so a NaN is never equal; the DLL says so).
fn eq_v2(a: [f32; 2], b: [f32; 2]) -> bool {
    a[0] == b[0] && a[1] == b[1]
}

/// EqualityComparer<string>.Default.GetHashCode: Mono's string hash, null as 0.
fn hash_string(s: Option<&str>) -> i32 {
    s.map_or(0, super::decal_renderer::string_hash)
}

/// Utils.Rotate (the pivot defaults to the origin): degrees to radians, then the rotation about the pivot. The result's z is 0.
pub fn utils_rotate(point: [f32; 2], degrees: f32, pivot: [f32; 2]) -> [f32; 3] {
    let degrees = degrees * (std::f32::consts::PI / 180.0);
    let p = [point[0] - pivot[0], point[1] - pivot[1]];
    let (c, s) = (mathf::cos(degrees), mathf::sin(degrees));
    let p = [(d(c) * p[0] - d(s) * p[1]).f(), (d(s) * p[0] + d(c) * p[1]).f()];
    [p[0] + pivot[0], p[1] + pivot[1], 0.0]
}

/// DamagePoint: the skin shader's Vector4 (x, y, intensity, DamageType).
#[derive(Default, Clone, Copy, Debug)]
pub struct DamagePoint {
    pub X: f32,
    pub Y: f32,
    pub Intensity: f32,
    /// DamageType as `misc::damage_type`.
    pub Type: i32,
}

impl DamagePoint {
    /// DamagePoint.None (the static setup)
    pub const NONE: DamagePoint = DamagePoint { X: 0.0, Y: 0.0, Intensity: 0.0, Type: super::misc::damage_type::NONE };

    /// DamagePoint.constructor
    pub fn new(x: f32, y: f32, intensity: f32, ty: i32) -> Self {
        DamagePoint { X: x, Y: y, Intensity: intensity, Type: ty }
    }

    /// DamagePoint.GetHashCode
    pub fn get_hash_code(&self) -> i32 {
        let k = -1521134295i32;
        let h = 252580816i32.wrapping_mul(k).wrapping_add(hash_f32(self.X));
        let h = h.wrapping_mul(k).wrapping_add(hash_f32(self.Y));
        let h = h.wrapping_mul(k).wrapping_add(hash_f32(self.Intensity));
        h.wrapping_mul(k).wrapping_add(self.Type)
    }

    /// DamagePoint.op_Implicit (to Vector4): (X, Y, Intensity, (float)Type)
    pub fn to_vector4(self) -> [f32; 4] {
        [self.X, self.Y, self.Intensity, self.Type as f32]
    }

    /// DamagePoint.op_Implicit (from Vector4): Type is Mathf.FloorToInt(w).
    pub fn from_vector4(a: [f32; 4]) -> Self {
        DamagePoint::new(a[0], a[1], a[2], super::translated::rt::to_int(a[3].floor() as f64))
    }
}

// DamagePoint.Equals (both overloads): the float fields compare with == (a NaN field is never equal), as PartialEq does.
// DamagePoint.op_Equality: PartialEq::eq
// DamagePoint.op_Inequality: PartialEq::ne
impl PartialEq for DamagePoint {
    fn eq(&self, o: &Self) -> bool {
        self.X == o.X && self.Y == o.Y && self.Intensity == o.Intensity && self.Type == o.Type
    }
}

/// RotatedRectangle
#[derive(Default, Clone, Copy, Debug)]
pub struct RotatedRectangle {
    pub Center: [f32; 2],
    pub Size: [f32; 2],
    pub Pivot: [f32; 2],
    pub AngleDegrees: f32,
}

impl RotatedRectangle {
    /// RotatedRectangle.constructor
    pub fn new(center: [f32; 2], size: [f32; 2], pivot: [f32; 2], angle_degrees: f32) -> Self {
        RotatedRectangle { Center: center, Size: size, Pivot: pivot, AngleDegrees: angle_degrees }
    }

    fn corner(&self, sx: f32, sy: f32) -> [f32; 2] {
        let r = utils_rotate(
            [(d(self.Center[0]) + sx * (self.Size[0] / 2.0)).f(), (d(self.Center[1]) + sy * (self.Size[1] / 2.0)).f()],
            self.AngleDegrees,
            self.Pivot,
        );
        [r[0], r[1]]
    }

    /// RotatedRectangle.GetTopLeft
    pub fn get_top_left(&self) -> [f32; 2] {
        self.corner(-1.0, 1.0)
    }

    /// RotatedRectangle.GetTopRight
    pub fn get_top_right(&self) -> [f32; 2] {
        self.corner(1.0, 1.0)
    }

    /// RotatedRectangle.GetBottomLeft
    pub fn get_bottom_left(&self) -> [f32; 2] {
        self.corner(-1.0, -1.0)
    }

    /// RotatedRectangle.GetBottomRight
    pub fn get_bottom_right(&self) -> [f32; 2] {
        self.corner(1.0, -1.0)
    }

    /// RotatedRectangle.GetHashCode (the pivot is not part of it)
    pub fn get_hash_code(&self) -> i32 {
        let k = -1521134295i32;
        let h = 1166163153i32.wrapping_mul(k).wrapping_add(hash_v2(self.Center));
        let h = h.wrapping_mul(k).wrapping_add(hash_v2(self.Size));
        h.wrapping_mul(k).wrapping_add(hash_f32(self.AngleDegrees))
    }
}

// RotatedRectangle.Equals: Center and Size by Vector2.Equals, AngleDegrees by ==; the Pivot is not compared.
// RotatedRectangle.op_Equality: PartialEq::eq
// RotatedRectangle.op_Inequality: PartialEq::ne
impl PartialEq for RotatedRectangle {
    fn eq(&self, o: &Self) -> bool {
        eq_v2(self.Center, o.Center) && eq_v2(self.Size, o.Size) && self.AngleDegrees == o.AngleDegrees
    }
}

/// LineSegment
#[derive(Clone, Copy, Debug, Default)]
pub struct LineSegment {
    pub A: [f32; 2],
    pub B: [f32; 2],
}

impl LineSegment {
    /// LineSegment.constructor
    pub fn new(a: [f32; 2], b: [f32; 2]) -> Self {
        LineSegment { A: a, B: b }
    }

    /// LineSegment.GetRandomPoint: Vector2.Lerp(A, B, Random.value)
    pub fn get_random_point(&self, random: &mut Random) -> [f32; 2] {
        let t = random.value();
        self.get_point_along(t)
    }

    /// LineSegment.GetPointAlong
    pub fn get_point_along(&self, progress: f32) -> [f32; 2] {
        mathf::v2_lerp(self.A, self.B, progress)
    }

    /// LineSegment.GetMidpoint: (A + B) * 0.5f
    pub fn get_midpoint(&self) -> [f32; 2] {
        [(self.A[0] + self.B[0]) * 0.5, (self.A[1] + self.B[1]) * 0.5]
    }

    /// LineSegment.GetDistance: Utils.DistanceFromPointToLineSegment
    pub fn get_distance(&self, point: [f32; 2]) -> f32 {
        super::blood_wire::distance_to_line_segment(point, self.A, self.B)
    }

    /// LineSegment.GetHashCode
    pub fn get_hash_code(&self) -> i32 {
        let k = -1521134295i32;
        (-1817952719i32).wrapping_mul(k).wrapping_add(hash_v2(self.A)).wrapping_mul(k).wrapping_add(hash_v2(self.B))
    }
}

// LineSegment.Equals (both overloads): A and B by Vector2.Equals.
// LineSegment.op_Equality: PartialEq::eq
// LineSegment.op_Inequality: PartialEq::ne
impl PartialEq for LineSegment {
    fn eq(&self, o: &Self) -> bool {
        eq_v2(self.A, o.A) && eq_v2(self.B, o.B)
    }
}

/// ComponentTagContainer.TaggedComponent: the component's instance id (None: a null Component) and its tag.
#[derive(Clone, Debug, Default)]
pub struct TaggedComponent {
    pub Component: Option<i32>,
    pub Tag: Option<String>,
}

impl TaggedComponent {
    /// TaggedComponent.GetHashCode; `component_hash` is the Component's GetHashCode (Mono's object hash, from the recording).
    pub fn get_hash_code(&self, component_hash: i32) -> i32 {
        let k = -1521134295i32;
        let c = if self.Component.is_some() { component_hash } else { 0 };
        951617459i32.wrapping_mul(k).wrapping_add(c).wrapping_mul(k).wrapping_add(hash_string(self.Tag.as_deref()))
    }
}

// TaggedComponent.Equals: the same Component object and an equal Tag string.
// TaggedComponent.op_Equality: PartialEq::eq
// TaggedComponent.op_Inequality: PartialEq::ne
impl PartialEq for TaggedComponent {
    fn eq(&self, o: &Self) -> bool {
        self.Component == o.Component && self.Tag == o.Tag
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_types() {
        let a = LineSegment::new([0.0, 0.0], [2.0, 4.0]);
        assert_eq!(a.get_midpoint(), [1.0, 2.0]);
        assert_eq!(a.get_point_along(0.5), [1.0, 2.0]);
        assert_eq!(a, LineSegment::new([0.0, 0.0], [2.0, 4.0]));
        assert_eq!(a.get_hash_code(), LineSegment::new([0.0, 0.0], [2.0, 4.0]).get_hash_code());
        assert_ne!(a, LineSegment::new([0.0, 0.0], [2.0, 5.0]));
        let nan = LineSegment::new([f32::NAN, 0.0], [0.0, 0.0]);
        // Vector2.Equals(Vector2) is x == x && y == y: a NaN is never equal, even to itself (the DLL agrees).
        assert_ne!(nan, nan);
        let p = DamagePoint::new(1.0, 2.0, 3.0, 4);
        assert_eq!(DamagePoint::from_vector4(p.to_vector4()), p);
        assert_eq!(DamagePoint::from_vector4([0.0, 0.0, 0.0, 5.9]), DamagePoint::NONE);
        assert_ne!(DamagePoint::new(f32::NAN, 0.0, 0.0, 0), DamagePoint::new(f32::NAN, 0.0, 0.0, 0));
        let r = RotatedRectangle::new([0.0, 0.0], [2.0, 2.0], [0.0, 0.0], 90.0);
        let tl = r.get_top_left();
        assert!((tl[0] + 1.0).abs() < 1e-5 && (tl[1] + 1.0).abs() < 1e-5);
        let mut r2 = r;
        r2.Pivot = [5.0, 5.0];
        assert_eq!(r, r2);
        let (t1, t2) = (TaggedComponent { Component: Some(7), Tag: Some("a".into()) }, TaggedComponent { Component: Some(7), Tag: Some("b".into()) });
        assert_ne!(t1, t2);
        assert_eq!((hash_string(Some("ab")), hash_string(None)), (1093630535, 0));
    }
}
