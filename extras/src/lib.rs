//! New content for the rewrite that the real game does not have. Nothing here is part of the 1:1 game: the main crate
//! asks for `ITEMS` at startup (src/extras_hook.rs) and adds them to the existing catalog categories, built from these
//! specs and art. A new item adds an `Item` here and its builder in the hook.

pub mod mishap;
pub mod portal;
pub mod sheet;

/// A catalog entry: the name the spawn menu and saves use, the existing category it joins, and its tooltip.
pub struct Item {
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    /// The catalog prefab this item is built from (its scripts and settings are reused, its parts replaced).
    pub template: &'static str,
}

pub const PORTAL_GUN: &str = "Portal Gun";

pub const ITEMS: &[Item] = &[
    Item {
        name: "Crab",
        category: "Entities",
        description: "A small, sturdy crab. It scuttles sideways, pinches whatever touches its claws and bleeds blue.",
        template: "Gorse",
    },
    Item {
        name: PORTAL_GUN,
        category: "Firearms",
        description: "Aperture Science Handheld Portal Device. Activate to fire a blue portal, hold Shift while activating to fire an orange one. The two portals are one doorway: things go through slowly or fast, halfway or all the way, keeping their speed.",
        template: "Pistol",
    },
];

/// The portal gun's art, drawn by `art/gen_art.py` at the game's 35 pixels per unit.
pub mod art {
    pub const PIXELS_PER_UNIT: f32 = 35.0;
    pub const GUN_ICON: &[u8] = include_bytes!("../art/portal_gun_icon.png");
    pub const GUN: &[u8] = include_bytes!("../art/portal_gun.png");
    /// White where the core and the top light glow, for the game to tint the last portal's colour.
    pub const GUN_CORE_GLOW: &[u8] = include_bytes!("../art/portal_gun_core.png");
    pub const LINE_BLUE: &[u8] = include_bytes!("../art/portal_line_blue.png");
    pub const LINE_ORANGE: &[u8] = include_bytes!("../art/portal_line_orange.png");
    /// The gun sprite's size in pixels, and its barrel tip and handle relative to the sprite's centre (pixels, y up).
    pub const GUN_SIZE: [f32; 2] = [32.0, 14.0];
    pub const GUN_TIP: [f32; 2] = [16.0, 0.5];
    pub const GUN_HANDLE: [f32; 2] = [-6.5, -5.5];
    /// The glass core's and the top light's centres (pixels from the sprite's centre).
    pub const GUN_CORE: [f32; 2] = [4.5, 0.0];
    pub const GUN_LIGHT: [f32; 2] = [-8.0, 6.5];
    /// A portal's opening is PORTAL_LEN game pixels along the surface (at Portal size 1x).
    pub const PORTAL_LEN: f32 = 84.0;
    /// A portal's oval (edge-on) spans the opening and is LINE_H game pixels across (its PNG is drawn at 4x density).
    pub const LINE_H: f32 = 5.0;
}

/// The crab's art (art/make_crab.py), PNG.
pub const CRAB_SKIN: &[u8] = include_bytes!("../art/crab-skin.png");
pub const CRAB_FLESH: &[u8] = include_bytes!("../art/crab-flesh.png");
pub const CRAB_BONE: &[u8] = include_bytes!("../art/crab-bone.png");
pub const CRAB_ICON: &[u8] = include_bytes!("../art/crab-icon.png");

/// Pixels per unit, the humans' density.
pub const PPU: f32 = 35.0;
/// The standing feet, in pixels below the shell's centre.
pub const FOOT: f32 = 12.0;
/// The crab's blood: liquid id and display name (src/game/circulation.rs registers it after the game's liquids).
pub const CRAB_BLOOD: &str = "CRAB BLOOD";
pub const CRAB_BLOOD_COLOUR: [f32; 4] = [0.05, 0.25, 0.85, 1.0];
/// Bruise, deeper bruise and wound-centre tints of the shell (the body shader's _BruiseColor, _Second.., _Third..).
pub const CRAB_BRUISES: [[f32; 4]; 3] = [[0.45, 0.12, 0.3, 1.0], [0.25, 0.1, 0.35, 1.0], [0.1, 0.15, 0.45, 1.0]];
pub const SPECIES: &str = "Crab";
/// The crab's muscle strength over the template's (LimbBehaviour.BaseStrength).
pub const STRENGTH: f32 = 4.0;
/// Shots on the shell weaker than this glance off (a pistol's do; a rifle's or a sniper's crack it).
pub const SHELL_GLANCE: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Shell,
    UpperLeg,
    LowerLeg,
    Arm,
    Claw,
    Pincer,
}

/// One limb. Positions are in the root's space, in units; every part but the shell hangs from its joint at the top
/// of its sprite and points along local -y.
#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    pub kind: Kind,
    /// -1 the crab's left (screen left), 1 its right; 0 the shell.
    pub side: f32,
    /// Leg pair 0 (front) or 1 (back).
    pub index: usize,
    pub sprite: &'static str,
    pub parent: Option<usize>,
    pub center: [f32; 2],
    /// Z rotation, degrees.
    pub angle: f32,
    pub size: [f32; 2],
    /// Hinge anchor in this part's space and in its parent's.
    pub anchor: [f32; 2],
    pub connected_anchor: [f32; 2],
    pub limits: [f32; 2],
    pub order: i16,
    /// Renderer colour: the far legs are darker, as the humans' back limbs.
    pub shade: f32,
    pub mass: f32,
}

pub fn rect(sprite: &str) -> [u32; 4] {
    sheet::RECTS.iter().find(|(n, _)| *n == sprite).unwrap_or_else(|| panic!("no crab sprite {sprite}")).1
}

/// The shell's collider outline (units, about its centre).
pub fn shell_outline() -> Vec<[f32; 2]> {
    [[-14.0, -3.0], [-14.0, 1.0], [-12.0, 3.5], [-9.0, 5.0], [-6.0, 6.0], [6.0, 6.0], [9.0, 5.0], [12.0, 3.5], [14.0, 1.0], [14.0, -3.0], [10.0, -6.5], [4.0, -8.0], [-4.0, -8.0], [-10.0, -6.5]]
        .iter()
        .map(|p| [p[0] / PPU, p[1] / PPU])
        .collect()
}

/// The crab's limbs, shell first, each after its parent, standing in its rest pose.
pub fn crab_parts() -> Vec<Part> {
    let px = 1.0 / PPU;
    let size = |s: &str| {
        let r = rect(s);
        [r[2] as f32 * px, r[3] as f32 * px]
    };
    let mut parts = vec![Part {
        name: "shell".into(),
        kind: Kind::Shell,
        side: 0.0,
        index: 0,
        sprite: "shell",
        parent: None,
        center: [0.0, 0.0],
        angle: 0.0,
        size: size("shell"),
        anchor: [0.0; 2],
        connected_anchor: [0.0; 2],
        limits: [0.0; 2],
        order: 0,
        shade: 1.0,
        mass: 0.12,
    }];
    // A part from joint `j` (root space) along `dir` (degrees from +x).
    let mut add = |parts: &mut Vec<Part>, name: String, kind, side, index, sprite: &'static str, parent: usize, j: [f32; 2], dir: f32, limits, order, shade, mass| -> [f32; 2] {
        let s = size(sprite);
        let d = [dir.to_radians().cos(), dir.to_radians().sin()];
        let reach = s[1] / 2.0 - px;
        let center = [j[0] + d[0] * reach, j[1] + d[1] * reach];
        let p = &parts[parent];
        let (sin, cos) = (-p.angle).to_radians().sin_cos();
        let rel = [j[0] - p.center[0], j[1] - p.center[1]];
        parts.push(Part {
            name,
            kind,
            side,
            index,
            sprite,
            parent: Some(parent),
            center,
            angle: dir + 90.0,
            size: s,
            anchor: [0.0, reach],
            connected_anchor: [rel[0] * cos - rel[1] * sin, rel[0] * sin + rel[1] * cos],
            limits,
            order,
            shade,
            mass,
        });
        [j[0] + d[0] * 2.0 * reach, j[1] + d[1] * 2.0 * reach]
    };
    for side in [-1.0f32, 1.0] {
        let lr = if side < 0.0 { "left" } else { "right" };
        // Legs, two a side as Ferris': the thigh out and a little up from under the shell's dark band, the shin bent
        // down and out to the floor FOOT px below the centre.
        for i in 0..2 {
            let attach = [side * (8.0 + 4.0 * i as f32) * px, -4.0 * px];
            let up = [15.0f32, 4.0][i];
            let thigh = parts.len();
            let (order, shade) = ([-1i16, -3][i], [1.0, 0.82][i]);
            let knee = add(&mut parts, format!("{lr} upper leg {i}"), Kind::UpperLeg, side, i, "upper leg", 0, attach, if side > 0.0 { up } else { 180.0 - up }, [-35.0, 35.0], order, shade, 0.015);
            let shin = size("lower leg")[1] - 2.0 * px;
            let fall = ((knee[1] + FOOT * px) / shin).clamp(-1.0, 1.0).asin().to_degrees();
            add(&mut parts, format!("{lr} lower leg {i}"), Kind::LowerLeg, side, i, "lower leg", thigh, knee, if side > 0.0 { -fall } else { 180.0 + fall }, [-35.0, 35.0], order - 1, shade, 0.015);
        }
        // Claw: the arm out and up from the shell's side, the claw raised, its moving finger (pincer) on the inner side.
        let arm = parts.len();
        let elbow = add(&mut parts, format!("{lr} arm"), Kind::Arm, side, 0, "arm", 0, [side * 12.0 * px, 0.0], if side > 0.0 { 45.0 } else { 135.0 }, [-60.0, 60.0], 2, 1.0, 0.015);
        let claw = parts.len();
        let sprite = if side > 0.0 { "claw mirrored" } else { "claw" };
        add(&mut parts, format!("{lr} claw"), Kind::Claw, side, 0, sprite, arm, elbow, 90.0 - side * 15.0, [-45.0, 45.0], 3, 1.0, 0.025);
        // The pincer's joint: the palm's inner bottom corner, in the claw's space (2 px in from the middle, at the palm's bottom row).
        let c = &parts[claw];
        let local = [side * 2.0 * px, 0.0];
        let (sin, cos) = c.angle.to_radians().sin_cos();
        let j = [c.center[0] + local[0] * cos - local[1] * sin, c.center[1] + local[0] * sin + local[1] * cos];
        add(&mut parts, format!("{lr} pincer"), Kind::Pincer, side, 0, "pincer", claw, j, 90.0 - side * 15.0, [-40.0, 40.0], 4, 1.0, 0.01);
    }
    parts
}

/// One limb's angle in a pose (LimbPose): a fixed angle, or Start..End over AnimationDuration from TimeOffset.
#[derive(Clone, Copy, Debug, Default)]
pub struct Angle {
    pub animated: bool,
    pub angle: f32,
    pub start: f32,
    pub end: f32,
    pub duration: f32,
    pub offset: f32,
}

fn fixed(angle: f32) -> Angle {
    Angle { angle, start: angle, end: angle, duration: 1.0, ..Default::default() }
}

fn swing(amp: f32, duration: f32, offset: f32) -> Angle {
    Angle { animated: true, angle: 0.0, start: -amp, end: amp, duration, offset }
}

/// The crab's angle for `part` in the pose named `pose` (the template's PoseState names). Angles are relative to the
/// rest stance `crab_parts` builds, positive counter-clockwise; the left side mirrors the right except when walking,
/// where every leg turns the same way (sideways scuttle, alternating diagonal pairs).
pub fn pose_angle(pose: &str, part: &Part) -> Angle {
    let s = part.side;
    // Diagonal pairs: the front leg of one side with the back leg of the other.
    let pair = if (part.index == 0) == (s > 0.0) { 0.0 } else { 0.5 };
    match (pose, part.kind) {
        ("Walking", Kind::UpperLeg) => swing(22.0, 1.6, pair),
        ("Walking", Kind::LowerLeg) => swing(18.0, 1.6, pair + 0.75),
        ("Walking", Kind::Arm) => fixed(s * 10.0),
        ("Protective", Kind::Arm) => fixed(s * 35.0),
        ("Protective", Kind::Claw) => fixed(s * 20.0),
        ("Protective", Kind::Pincer) => fixed(s * 25.0),
        ("Protective" | "Sitting" | "Flat", Kind::UpperLeg) => fixed(s * 15.0),
        ("Protective" | "Sitting" | "Flat", Kind::LowerLeg) => fixed(s * -20.0),
        ("Flailing" | "WrithingInPain" | "Brain damage", Kind::UpperLeg | Kind::LowerLeg) => swing(30.0, 0.35, part.index as f32 * 0.3 + if s > 0.0 { 0.5 } else { 0.0 }),
        ("Flailing" | "WrithingInPain", Kind::Arm) => swing(40.0, 0.5, if s > 0.0 { 0.5 } else { 0.0 }),
        ("Flailing" | "WrithingInPain", Kind::Pincer) => swing(20.0, 0.25, 0.0),
        ("Swimming", Kind::UpperLeg | Kind::LowerLeg) => swing(25.0, 0.8, part.index as f32 * 0.5),
        _ => fixed(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every leg's foot reaches the same floor, and each part's joint lies where its parent's anchor says.
    #[test]
    fn stance() {
        let parts = crab_parts();
        assert_eq!(parts.len(), 1 + 8 + 6);
        for p in &parts {
            let Some(q) = p.parent.map(|i| &parts[i]) else { continue };
            let world = |c: [f32; 2], a: f32, l: [f32; 2]| {
                let (s, k) = a.to_radians().sin_cos();
                [c[0] + l[0] * k - l[1] * s, c[1] + l[0] * s + l[1] * k]
            };
            let a = world(p.center, p.angle, p.anchor);
            let b = world(q.center, q.angle, p.connected_anchor);
            assert!((a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5, "{}: {a:?} vs {b:?}", p.name);
            if p.kind == Kind::LowerLeg {
                let foot = world(p.center, p.angle, [0.0, -p.anchor[1]]);
                assert!((foot[1] + FOOT / PPU).abs() < 1e-4, "{} foot at {}", p.name, foot[1] * PPU);
            }
        }
    }
}
