//! Portal maths in 2D. The pair mapping is Stink-O's Portal 2 Rust Engine's `PortalPair` (p2_core/src/math.rs, MIT
//! at commit 76c4767, see THIRD_PARTY_NOTICES.md) brought down to the plane: whatever enters one opening leaves the
//! other as if the two were glued back to back. In 3D that is `exit * FLIP(180 deg about the portal's up) * entry^-1`;
//! in 2D the portal's "up" is the axis out of the screen, so the flip is a half turn and the whole mapping is the
//! rotation by `exit angle - entry angle + pi` about the openings' centres (a proper rotation: nothing is mirrored).

pub type V = [f32; 2];

/// Half the length of an opening, in world units (the sprite's 84 px oval at 35 px per unit).
pub const HALF_LEN: f32 = crate::art::PORTAL_LEN / crate::art::PIXELS_PER_UNIT / 2.0;

fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1]]
}
pub fn dot(a: V, b: V) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}
/// `v` turned a quarter left.
pub fn perp(v: V) -> V {
    [-v[1], v[0]]
}
pub fn rotate(v: V, angle: f32) -> V {
    let (s, c) = angle.sin_cos();
    [c * v[0] - s * v[1], s * v[0] + c * v[1]]
}

/// An opening: its centre on the surface and the unit normal out of the surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub centre: V,
    pub normal: V,
}

impl Frame {
    /// Along the surface (the normal turned a quarter right).
    pub fn tangent(&self) -> V {
        [self.normal[1], -self.normal[0]]
    }
    pub fn angle(&self) -> f32 {
        self.normal[1].atan2(self.normal[0])
    }
    /// A world point as (along the surface, in front of it).
    pub fn local(&self, p: V) -> V {
        let d = sub(p, self.centre);
        [dot(d, self.tangent()), dot(d, self.normal)]
    }
}

/// The mapping from `entry` to `exit`.
#[derive(Clone, Copy, Debug)]
pub struct Pair {
    pub entry: Frame,
    pub exit: Frame,
    /// The rotation (radians) that takes entry-side directions to exit-side ones.
    pub rotation: f32,
}

impl Pair {
    pub fn new(entry: Frame, exit: Frame) -> Self {
        Pair { entry, exit, rotation: exit.angle() - entry.angle() + std::f32::consts::PI }
    }
    /// Done relative to the frames, as Stink-O's map_point, so it stays precise far from the origin.
    pub fn map_point(&self, p: V) -> V {
        add(self.exit.centre, rotate(sub(p, self.entry.centre), self.rotation))
    }
    pub fn map_dir(&self, v: V) -> V {
        rotate(v, self.rotation)
    }
}

/// The velocity of a rigid body's point `p` (linear velocity `v` at its centre of mass `com`, `w` radians a second).
pub fn point_velocity(v: V, w: f32, com: V, p: V) -> V {
    let r = perp(sub(p, com));
    [v[0] + w * r[0], v[1] + w * r[1]]
}

/// A body's velocity through a pair whose surfaces move: relative to the entry surface going in, plus the exit
/// surface's own velocity coming out ("speedy thing goes in, speedy thing comes out", seen from the moving openings).
/// `entry_surface` is the entry surface's velocity at the body's centre, `exit_surface` the exit's at the mapped centre.
pub fn exit_velocity(pair: &Pair, v: V, entry_surface: V, exit_surface: V) -> V {
    add(pair.map_dir(sub(v, entry_surface)), exit_surface)
}

/// How far behind an opening's surface a body can reach and still be in the doorway (the hole goes this deep).
pub const DEPTH: f32 = 4.0;

/// Clips the triangle `t` (each vertex's side `s`: positive in front) to the front side, as 0, 1 or 2 triangles of
/// (vertex index a, vertex index b, weight of b) blends: a corner kept is (i, i, 0).
pub fn clip_triangle(s: [f32; 3]) -> Vec<[(usize, usize, f32); 3]> {
    let mut poly: Vec<(usize, usize, f32)> = Vec::new();
    for i in 0..3 {
        let j = (i + 1) % 3;
        if s[i] >= 0.0 {
            poly.push((i, i, 0.0));
        }
        if (s[i] >= 0.0) != (s[j] >= 0.0) {
            poly.push((i, j, s[i] / (s[i] - s[j])));
        }
    }
    (1..poly.len().saturating_sub(1)).map(|k| [poly[0], poly[k], poly[k + 1]]).collect()
}

/// The shifts along the surface tried, nearest first, when the opening does not fit where the shot landed (Portal
/// slides a portal off a ledge or a corner when it can).
pub fn fit_shifts() -> impl Iterator<Item = f32> {
    let step = HALF_LEN / 6.0;
    std::iter::once(0.0).chain((1..=6).flat_map(move |k| [k as f32 * step, -(k as f32) * step]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn close(a: V, b: V) -> bool {
        (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
    }

    #[test]
    fn opposite_walls_are_a_corridor() {
        // Into a wall facing +x at x=0, out of a wall facing -x at x=10: keep going +x.
        let pair = Pair::new(Frame { centre: [0.0, 0.0], normal: [1.0, 0.0] }, Frame { centre: [10.0, 0.0], normal: [-1.0, 0.0] });
        assert!(close(pair.map_dir([-3.0, 1.0]), [-3.0, 1.0]));
        // A point 0.5 in front of the entry ends 0.5 behind the exit, at the same height.
        assert!(close(pair.map_point([0.5, 0.2]), [10.5, 0.2]));
    }

    #[test]
    fn same_facing_walls_turn_you_around() {
        let wall = |x| Frame { centre: [x, 0.0], normal: [1.0, 0.0] };
        let pair = Pair::new(wall(0.0), wall(5.0));
        assert!(close(pair.map_dir([-2.0, 0.0]), [2.0, 0.0]));
    }

    #[test]
    fn floor_to_wall_fling_keeps_speed() {
        // Falling into a floor portal, out of a wall facing +x: the fall becomes a throw to the right.
        let pair = Pair::new(Frame { centre: [0.0, 0.0], normal: [0.0, 1.0] }, Frame { centre: [-5.0, 2.0], normal: [1.0, 0.0] });
        assert!(close(pair.map_dir([0.0, -7.0]), [7.0, 0.0]));
        assert!((pair.rotation.rem_euclid(2.0 * PI) - 0.5 * PI).abs() < 1e-5);
        // The entry and exit swap under the inverse pair.
        let back = Pair::new(pair.exit, pair.entry);
        assert!(close(back.map_point(pair.map_point([0.3, -0.1])), [0.3, -0.1]));
    }

    #[test]
    fn moving_surfaces_add_their_speed() {
        // A floor portal rising at 4 under a box at rest: the box goes in at 4 and comes out of a still wall at 4.
        let pair = Pair::new(Frame { centre: [0.0, 0.0], normal: [0.0, 1.0] }, Frame { centre: [-5.0, 2.0], normal: [1.0, 0.0] });
        assert!(close(exit_velocity(&pair, [0.0, 0.0], [0.0, 4.0], [0.0, 0.0]), [4.0, 0.0]));
        // Spinning at 2 rad/s about the origin, the point (1, 0) moves up at 2.
        assert!(close(point_velocity([0.0, 0.0], 2.0, [0.0, 0.0], [1.0, 0.0]), [0.0, 2.0]));
    }

    #[test]
    fn clipping() {
        assert_eq!(clip_triangle([1.0, 1.0, 1.0]).len(), 1);
        assert!(clip_triangle([-1.0, -1.0, -1.0]).is_empty());
        // One corner behind: a quad, two triangles; the cut is halfway along each edge to it.
        let t = clip_triangle([1.0, 1.0, -1.0]);
        assert_eq!(t.len(), 2);
        assert!(t.iter().flatten().any(|&(a, b, w)| (a, b) == (1, 2) && w == 0.5));
        assert_eq!(clip_triangle([1.0, -1.0, -1.0]).len(), 1);
        assert_eq!(fit_shifts().count(), 13);
    }
}
