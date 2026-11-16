//! Portal mishaps: what a portal shot does to a person (human or android) it hits, instead of opening. The
//! "fantastical quantum tunnelling device" fails in one of three ways, each with its own toggle in Settings -> Mods:
//! superposition (two flickering copies that collapse after a while), a spatial fold (a limb comes out of another
//! one through tiny portals) and phase loss (the person drifts through things until each limb snaps back).
//! The game side is src/game/mishap_glue.rs; this is the part that needs no game.

use crate::portal::{rotate, Frame, V};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Superposition,
    Fold,
    Phase,
}

pub const EFFECTS: [Effect; 3] = [Effect::Superposition, Effect::Fold, Effect::Phase];

/// One of the effects that are on, at random (`r` in 0..1); none on: the shot fizzles.
pub fn pick(on: [bool; 3], r: f32) -> Option<Effect> {
    let live: Vec<Effect> = EFFECTS.iter().zip(on).filter(|e| e.1).map(|e| *e.0).collect();
    (!live.is_empty()).then(|| live[((r * live.len() as f32) as usize).min(live.len() - 1)])
}

/// How a superposition ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Collapse {
    /// The copies snap back into one (dazed, glitching for a while).
    Merge,
    /// Both stay real.
    Clone,
    /// The copies are pulled toward each other across the gap, stretching, until they tear apart.
    Tear,
}

/// Usually a merge, sometimes a clone, rarely the catastrophe.
pub fn collapse(r: f32) -> Collapse {
    match r {
        r if r < 0.65 => Collapse::Merge,
        r if r < 0.9 => Collapse::Clone,
        _ => Collapse::Tear,
    }
}

/// How long a superposition lasts (game seconds), at random: about five.
pub fn split_time(r: f32) -> f32 {
    4.5 + r
}
/// The dazed glitching after a merge, and the stretching before a tear (game seconds).
pub const DAZED_TIME: f32 = 4.0;
pub const TEAR_TIME: f32 = 1.6;
/// When a phased limb snaps back into reality, after the hit (game seconds), at random.
pub fn snap_time(r: f32) -> f32 {
    0.8 + 3.2 * r
}
/// A tiny fold portal's length (world units).
pub const RIM_LEN: f32 = 0.42;

/// A fold: whatever sticks out of `from` (a point on a surface, its normal pointing out) sticks out of `to` instead,
/// as rigidly as before. Unlike a portal pair (in at one, out of the other), both sides face out: the rotation that
/// turns `from`'s normal into `to`'s, about the two points.
#[derive(Clone, Copy, Debug)]
pub struct Fold {
    pub from: Frame,
    pub to: Frame,
    pub rotation: f32,
}

impl Fold {
    pub fn new(from: Frame, to: Frame) -> Self {
        Fold { from, to, rotation: to.angle() - from.angle() }
    }
    pub fn map_point(&self, p: V) -> V {
        let d = rotate([p[0] - self.from.centre[0], p[1] - self.from.centre[1]], self.rotation);
        [self.to.centre[0] + d[0], self.to.centre[1] + d[1]]
    }
    pub fn map_dir(&self, v: V) -> V {
        rotate(v, self.rotation)
    }
}

/// The reference angle that keeps a hinge's angle (`angle_b - angle_a - reference`) at `angle` with the bodies at the
/// new angles: a joint evaluated through a fold keeps the bend it had.
pub fn keep_reference(angle: f32, angle_a: f32, angle_b: f32) -> f32 {
    angle_b - angle_a - angle
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: V, b: V) -> bool {
        (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
    }

    #[test]
    fn picking() {
        assert_eq!(pick([false; 3], 0.5), None);
        assert_eq!(pick([true, false, false], 0.99), Some(Effect::Superposition));
        assert_eq!(pick([false, true, true], 0.0), Some(Effect::Fold));
        assert_eq!(pick([false, true, true], 0.99), Some(Effect::Phase));
        assert_eq!(pick([true; 3], 1.0), Some(Effect::Phase));
        assert_eq!(collapse(0.1), Collapse::Merge);
        assert_eq!(collapse(0.7), Collapse::Clone);
        assert_eq!(collapse(0.95), Collapse::Tear);
    }

    #[test]
    fn fold_moves_what_sticks_out() {
        // An arm hanging down from a shoulder (normal down) comes out of a back facing -x instead.
        let fold = Fold::new(Frame { centre: [0.0, 0.0], normal: [0.0, -1.0] }, Frame { centre: [5.0, 1.0], normal: [-1.0, 0.0] });
        assert!(close(fold.map_point([0.0, -0.5]), [4.5, 1.0]));
        assert!(close(fold.map_dir([0.0, -1.0]), [-1.0, 0.0]));
        assert!(close(fold.map_point([0.0, 0.0]), [5.0, 1.0]));
        // The hinge keeps its bend: 0.3 rad before, 0.3 rad after with both bodies turned.
        let r = keep_reference(0.3, 1.0 + fold.rotation, 2.0);
        assert!((2.0 - (1.0 + fold.rotation) - r - 0.3).abs() < 1e-6);
    }
}
