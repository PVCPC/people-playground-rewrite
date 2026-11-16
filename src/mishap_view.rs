//! Extras (not the game's): the portal mishaps' drawing (src/game/mishap_glue.rs is the game side). A split's two
//! copies and a phased person's limbs are see-through and flicker in the shot's colour; a dazed person glitches (the
//! sprites jump and flash now and then); a fold's tiny portals are small glowing ovals where the limb goes in and
//! comes out; a tear stretches glowing threads between the copies; flashes of light where things happen.

use crate::limb_material::{Body, LegacyBody, LimbMaterial};
use crate::play::{LimbSprite, Play, Textures};
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use ppg_extras::art;
use ppg_extras::mishap::RIM_LEN;
use ppg_extras::portal::Frame;

pub(crate) const COLOURS: [Vec3; 2] = [Vec3::new(0.18, 0.55, 1.0), Vec3::new(1.0, 0.48, 0.07)];
const PPU: f32 = art::PIXELS_PER_UNIT;

/// A limb sprite drawn as a mishap's: its own tint before.
#[derive(Component)]
pub struct Tinted(Vec4);

/// A fold's tiny portal (k-th rim): 0 the oval, 1 its light.
#[derive(Component)]
#[require(crate::play::MainScene)]
pub struct Rim(usize, u8);

/// A tear's thread (k-th pair): 0 the line, 1 its light.
#[derive(Component)]
#[require(crate::play::MainScene)]
pub struct Thread(usize, u8);

#[derive(Component)]
#[require(crate::play::MainScene)]
pub struct Glint {
    life: f32,
    max: f32,
    colour: Vec4,
}

pub struct Art {
    quad: Handle<Mesh>,
    line: [Handle<Image>; 2],
    soft: Handle<Image>,
}

/// The game's light sprite texture (HDParticle), as extras_view's portals use.
pub(crate) fn game_light(play: &Play, images: &mut Assets<Image>, textures: &mut Textures) -> Handle<Image> {
    let found = play.game.assets.as_deref().and_then(|db| {
        let r = db.sprite_by_name("HDParticle")?;
        let tex = db.resolve(r.file, db.sprite(r).ok()?.texture)?;
        crate::play::load_texture(db, images, textures, tex)
    });
    found.map_or_else(|| images.add(Image::default()), |(h, _)| h)
}

fn frame_transform(f: Frame, z: f32) -> Transform {
    let t = f.tangent();
    Transform::from_xyz(f.centre[0], f.centre[1], z).with_rotation(Quat::from_rotation_z(t[1].atan2(t[0])))
}

/// A cheap hash to 0..1 (the flicker's dice; drawing only).
pub(crate) fn noise(a: u32, b: u32) -> f32 {
    let mut x = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// After extras_view's doorway: the mishaps' looks.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn draw(
    mut commands: Commands,
    mut play: ResMut<Play>,
    time: Res<Time>,
    mut images: ResMut<Assets<Image>>,
    mut textures: ResMut<Textures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut glows: ResMut<Assets<crate::lights::Glow>>,
    mut bodies: ResMut<Assets<Body>>,
    mut legacies: ResMut<Assets<LegacyBody>>,
    mut colors: ResMut<Assets<ColorMaterial>>,
    mut view: Local<Option<Art>>,
    mut frame: Local<u32>,
    mut sprites: Query<(Entity, &LimbSprite, &mut Transform, Option<&LimbMaterial>, Option<&MeshMaterial2d<ColorMaterial>>, Option<&Tinted>), (Without<Rim>, Without<Thread>, Without<Glint>)>,
    mut rims: Query<(Entity, &Rim, &mut Transform, &MeshMaterial2d<crate::lights::Glow>), (Without<LimbSprite>, Without<Thread>, Without<Glint>)>,
    mut threads: Query<(Entity, &Thread, &mut Transform, &MeshMaterial2d<crate::lights::Glow>), (Without<LimbSprite>, Without<Rim>, Without<Glint>)>,
    mut glints: Query<(Entity, &mut Glint, &mut Transform, &MeshMaterial2d<crate::lights::Glow>), (Without<LimbSprite>, Without<Rim>, Without<Thread>)>,
) {
    if view.is_none() {
        let soft = game_light(&play, &mut images, &mut textures);
        let line = [art::LINE_BLUE, art::LINE_ORANGE].map(|b| images.add(crate::extras_view::png_image(b)));
        *view = Some(Art { quad: meshes.add(Rectangle::new(1.0, 1.0)), line, soft });
    }
    let v = view.as_ref().unwrap();
    *frame = frame.wrapping_add(1);
    let f = *frame;
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let g = &mut play.game;

    // The limbs: see-through and flickering (split, phased) or glitching (dazed).
    let looks = g.mishap_looks();
    for (e, &LimbSprite(l), mut tf, material, plain, tinted) in &mut sprites {
        let look = looks.get(&l).copied();
        let current = |bodies: &Assets<Body>, legacies: &Assets<LegacyBody>, colors: &Assets<ColorMaterial>| match material {
            Some(LimbMaterial::Body(h)) => bodies.get(h).map(|m| m.extra.tint),
            Some(LimbMaterial::Legacy(h)) => legacies.get(h).map(|m| m.extra.tint),
            None => plain.and_then(|p| colors.get(&p.0)).map(|m| m.color.to_linear().to_vec4()),
        };
        let Some((colour, ghost)) = look else {
            if let Some(&Tinted(orig)) = tinted {
                set_tint(material, plain, &mut bodies, &mut legacies, &mut colors, orig);
                commands.entity(e).remove::<Tinted>();
            }
            continue;
        };
        let orig = match tinted {
            Some(&Tinted(o)) => o,
            None => {
                let Some(o) = current(&bodies, &legacies, &colors) else { continue };
                commands.entity(e).insert(Tinted(o));
                o
            }
        };
        let c = COLOURS[colour];
        let tint = if ghost {
            // A shimmer, and now and then a frame where it almost goes.
            let shimmer = 0.42 + 0.16 * (now * 21.0 + l as f32 * 1.7).sin();
            let alpha = if noise(f / 2, l as u32) < 0.12 { 0.12 } else { shimmer };
            (orig.truncate() * 0.55 + c * 0.75).extend(orig.w * alpha)
        } else {
            // Dazed: mostly itself; a glitch frame jumps the sprite and washes it in the colour.
            let glitch = noise(f / 3, 7 + l as u32 / 4) < 0.18;
            if glitch {
                let (dx, dy) = (noise(f, l as u32) - 0.5, noise(f, l as u32 + 99) - 0.5);
                tf.translation.x += dx * 0.14;
                tf.translation.y += dy * 0.06;
                (orig.truncate() * 0.4 + c * 0.9).extend(orig.w * 0.75)
            } else {
                orig
            }
        };
        set_tint(material, plain, &mut bodies, &mut legacies, &mut colors, tint);
    }

    // The fold's tiny portals: an oval and a soft light at each end.
    let rim_list = g.mishap_rims();
    let have = rims.iter().filter(|r| r.1 .1 == 0).count();
    for k in have..rim_list.len() {
        for part in [0u8, 1] {
            let (tex, layer) = if part == 0 { (v.line[rim_list[k].1].clone(), 0) } else { (v.soft.clone(), 1) };
            let m = glows.add(crate::lights::Glow { color: Vec4::ZERO, texture: tex });
            commands.spawn((Rim(k, part), Mesh2d(v.quad.clone()), MeshMaterial2d(m), Transform::default(), RenderLayers::layer(layer)));
        }
    }
    for (e, &Rim(k, part), mut tf, material) in &mut rims {
        let Some(&(frame, colour)) = rim_list.get(k) else {
            commands.entity(e).despawn();
            continue;
        };
        let pulse = 1.0 + 0.15 * (now * 7.0 + k as f32).sin();
        *tf = frame_transform(frame, 0.7).with_scale(if part == 0 { Vec3::new(RIM_LEN, 4.0 / PPU, 1.0) } else { Vec3::new(RIM_LEN * 2.2 * pulse, 0.6 * pulse, 1.0) });
        if let Some(mut m) = glows.get_mut(&material.0) {
            if part == 0 {
                m.texture = v.line[colour].clone();
                m.color = Vec4::splat(1.8 * pulse);
            } else {
                m.color = COLOURS[colour].extend(1.0) * 1.3;
            }
        }
    }

    // A tear's threads: stretched, flickering lines between each limb and its copy.
    let tears = g.mishap_tears();
    let have = threads.iter().filter(|t| t.1 .1 == 0).count();
    for k in have..tears.len() {
        for part in [0u8, 1] {
            let (tex, layer) = if part == 0 { (v.line[tears[k].2].clone(), 0) } else { (v.soft.clone(), 1) };
            let m = glows.add(crate::lights::Glow { color: Vec4::ZERO, texture: tex });
            commands.spawn((Thread(k, part), Mesh2d(v.quad.clone()), MeshMaterial2d(m), Transform::default(), RenderLayers::layer(layer)));
        }
    }
    for (e, &Thread(k, part), mut tf, material) in &mut threads {
        let Some(&(a, b, colour)) = tears.get(k) else {
            commands.entity(e).despawn();
            continue;
        };
        let (a, b) = (Vec2::from(a), Vec2::from(b));
        let d = b - a;
        let mid = (a + b) / 2.0;
        let flick = 0.6 + 0.8 * noise(f, k as u32 + 3);
        *tf = Transform::from_translation(mid.extend(0.7)).with_rotation(Quat::from_rotation_z(d.y.atan2(d.x))).with_scale(if part == 0 { Vec3::new(d.length(), 3.0 / PPU, 1.0) } else { Vec3::new(d.length() + 0.6, 0.7, 1.0) });
        if let Some(mut m) = glows.get_mut(&material.0) {
            m.color = if part == 0 { Vec4::splat(1.6 * flick) } else { COLOURS[colour].extend(1.0) * 0.8 * flick };
        }
    }

    // Flashes of light, fading.
    for (at, colour, size) in std::mem::take(&mut g.mishap.flashes) {
        let c = COLOURS[colour].extend(1.0) * 2.2;
        let m = glows.add(crate::lights::Glow { color: c, texture: v.soft.clone() });
        commands.spawn((Glint { life: 0.4, max: 0.4, colour: c }, Mesh2d(v.quad.clone()), MeshMaterial2d(m), Transform::from_translation(Vec2::from(at).extend(0.0)).with_scale(Vec3::splat(size)), RenderLayers::layer(1)));
    }
    for (e, mut glint, mut tf, material) in &mut glints {
        glint.life -= dt;
        if glint.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let k = glint.life / glint.max;
        if let Some(mut m) = glows.get_mut(&material.0) {
            m.color = glint.colour * k * k;
        }
        tf.scale *= 1.0 + 0.8 * dt / glint.max;
    }
}

fn set_tint(material: Option<&LimbMaterial>, plain: Option<&MeshMaterial2d<ColorMaterial>>, bodies: &mut Assets<Body>, legacies: &mut Assets<LegacyBody>, colors: &mut Assets<ColorMaterial>, tint: Vec4) {
    match material {
        Some(LimbMaterial::Body(h)) => {
            if let Some(mut m) = bodies.get_mut(h) {
                m.extra.tint = tint;
            }
        }
        Some(LimbMaterial::Legacy(h)) => {
            if let Some(mut m) = legacies.get_mut(h) {
                m.extra.tint = tint;
            }
        }
        None => {
            if let Some(mut m) = plain.and_then(|p| colors.get_mut(&p.0)) {
                m.color = Color::LinearRgba(LinearRgba::from_vec4(tint));
            }
        }
    }
}

/// PP_MISHAP=<scene> (with PPG_SHOT and --play-map Default, the mishaps' check): a gun on the floor fires an orange
/// portal into the left wall, a Human is dropped at (3, 2.6), and at 6 s a blue shot from the right hits one of its limbs
/// with the scene's mishap: split (a superposition left alone), merge / clone / tear (one that collapses that way at
/// 7.5 s), violent (hit again at 7 s), fold (the head hit, then a lower arm at 6.6 s and the head again at 7.2 s), phase, phase-crate
/// (a Crate at (1.7, 0.8), the phased person drifting left into it). Logs MISHAP lines.
pub fn add_test(app: &mut App, scene: String) {
    use crate::game::Game;
    use ppg_extras::mishap::{Collapse, Effect};
    fn spawn(g: &mut Game, name: &str, at: [f32; 2], flipped: bool) -> usize {
        let mut db = g.assets.take().expect("the game's data");
        let first = g.physicals.len();
        let (inst, _) = g.catalog_spawn(&mut db, name, [at[0], at[1], 0.0], flipped).expect("PP_MISHAP: the spawn");
        g.assets = Some(db);
        g.unsynced.push(inst);
        first
    }
    /// A blue shot from the right at the human's limb named `name` (its centre's height), as the gun would land it.
    fn shoot(g: &mut Game, first: usize, name: &str) {
        let Some(p) = (first..g.physicals.len()).find(|&p| g.physicals[p].name == name && !g.physicals[p].destroyed) else { return };
        let c = g.world.body(g.physicals[p].rigidbody.body).sweep.c;
        let from = [c.x + 3.0, c.y];
        let skip: Vec<_> = g.extras.guns.iter().map(|gun| g.physicals[gun.p].rigidbody.body).collect();
        let done = g.extras_place(0, from, [-1.0, 0.0], &skip);
        let to = [c.x, c.y];
        g.extras.fx.push(crate::game::extras_glue::Fx::Fire { colour: 0, from, to });
        eprintln!("MISHAP shot {name} at ({:.2}, {:.2}) -> {done}", c.x, c.y);
    }
    let (effect, collapse) = match scene.as_str() {
        "split" | "violent" => (Effect::Superposition, None),
        "merge" => (Effect::Superposition, Some(Collapse::Merge)),
        "clone" => (Effect::Superposition, Some(Collapse::Clone)),
        "tear" => (Effect::Superposition, Some(Collapse::Tear)),
        "fold" => (Effect::Fold, None),
        _ => (Effect::Phase, None),
    };
    app.add_systems(Update, move |time: Res<Time>, play: Option<ResMut<Play>>, mut step: Local<u32>, mut first: Local<usize>, mut last_log: Local<f32>| {
        let Some(mut play) = play else { return };
        let g = &mut play.game;
        let t = time.elapsed_secs();
        match *step {
            0 if t >= 4.0 => {
                *step = 1;
                spawn(g, ppg_extras::PORTAL_GUN, [7.0, 0.4], true);
            }
            1 if t >= 4.3 => {
                *step = 2;
                g.extras_fire(0, 1);
                *first = spawn(g, "Human", [3.0, 2.6], false);
                if scene == "phase-crate" {
                    spawn(g, "Crate", [1.7, 0.8], false);
                }
            }
            2 if t >= 6.0 => {
                *step = 3;
                g.mishap.force = Some(effect);
                g.mishap.force_collapse = collapse;
                shoot(g, *first, if effect == Effect::Fold { "Head" } else { "UpperBody" });
                if scene == "phase-crate" {
                    for p in &mut g.mishap.phases {
                        p.drift = -0.8;
                    }
                }
                if collapse.is_some() {
                    for s in &mut g.mishap.splits {
                        s.ends = g.time.time + 1.5;
                    }
                }
            }
            3 if t >= 6.6 && scene == "fold" => {
                *step = 4;
                shoot(g, *first, "LowerArmFront");
            }
            4 if t >= 7.2 && scene == "fold" => {
                *step = 5;
                // The head itself again (a shot from the side could hit another limb first): it folds anew.
                if let Some(p) = (*first..g.physicals.len()).find(|&p| g.physicals[p].name == "Head" && !g.physicals[p].destroyed) {
                    let b = g.physicals[p].rigidbody.body;
                    let c = g.world.body(b).sweep.c;
                    eprintln!("MISHAP refold Head -> {:?}", g.mishap_hit(0, b, [c.x, c.y]));
                }
            }
            3 if t >= 7.0 && scene == "violent" => {
                *step = 4;
                shoot(g, *first, "UpperBody");
            }
            _ => {}
        }
        if *step >= 3 && t - *last_log >= 0.25 {
            *last_log = t;
            let m = &g.mishap;
            let gone = g.physicals.iter().filter(|p| p.isDisintegrated).count();
            let joints = (*first..g.physicals.len()).filter(|&p| !g.physicals[p].destroyed).count();
            eprintln!(
                "MISHAP t={t:.2} game={:.2} people={} splits={:?} folds={} phased={:?} dazed={} disintegrated={gone} live_physicals_from_human={joints}",
                g.time.time,
                g.people.iter().filter(|p| !p.destroyed).count(),
                m.splits.iter().map(|s| (s.person, s.copy, s.tear.is_some(), s.ends)).collect::<Vec<_>>(),
                g.mishap_rims().len() / 2,
                m.phases.iter().map(|p| p.limbs.len()).collect::<Vec<_>>(),
                m.dazed.len()
            );
        }
    });
}
