//! Extras (not the game's): the portal gun's drawing. The gun (its glass core and top light glow in the last portal's
//! colour), each portal as a thin glowing oval on its surface with a soft light on the lights layer, the shot's beam and the flashes where portals open, close or
//! fizzle (the particles are the game's own prefabs, played by the glue), and the doorway's "view through": a sprite in
//! a doorway is clipped at the entrance surface and its twin, clipped to the exit side, is drawn there.

use crate::game::extras_glue::Fx;
use crate::play::{LimbSprite, ObjectSprite, Play, RendererSprite, Textures};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::{PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use ppg_extras::art;
use ppg_extras::portal::{clip_triangle, Frame, HALF_LEN};

/// An extras PNG (RGBA, 8 bits) as an Image.
pub fn png_image(bytes: &[u8]) -> Image {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().expect("an extras PNG");
    let mut buf = vec![0; reader.output_buffer_size().expect("an extras PNG")];
    let info = reader.next_frame(&mut buf).expect("an extras PNG");
    buf.truncate(info.buffer_size());
    Image::new(
        bevy::render::render_resource::Extent3d { width: info.width, height: info.height, depth_or_array_layers: 1 },
        bevy::render::render_resource::TextureDimension::D2,
        buf,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// A gun's part: 0 its sprite (the Brick's is hidden), 1 the core's glow, 2 the core's light, 3 the top light's light.
#[derive(Component)]
#[require(crate::play::MainScene)]
pub struct GunSprite(usize, u8);

/// A portal's part: 0 the line, 1 its light (lights layer).
#[derive(Component)]
#[require(crate::play::MainScene)]
pub struct PortalLine(usize, u8);

/// A light or beam that fades out (its own material).
#[derive(Component)]
#[require(crate::play::MainScene)]
pub struct Flash {
    life: f32,
    max: f32,
    colour: Vec4,
    grow: f32,
}

/// A sprite in a doorway: its own mesh, the clipped one it shows, and its twin (kept while it is in the doorway, its
/// meshes rewritten each frame).
#[derive(Component)]
pub struct Clipped {
    orig: Handle<Mesh>,
    shown: Handle<Mesh>,
    twin: Entity,
    twin_mesh: Handle<Mesh>,
}

#[derive(Component)]
#[require(crate::play::MainScene)]
pub struct Twin;

pub struct ViewArt {
    gun_mesh: Handle<Mesh>,
    core: [Handle<crate::lights::Glow>; 2],
    line_mesh: Handle<Mesh>,
    quad: Handle<Mesh>,
    line: [Handle<crate::lights::Glow>; 2],
    light: [Handle<crate::lights::Glow>; 2],
    line_tex: [Handle<Image>; 2],
    soft: Handle<Image>,
}

const COLOURS: [Vec3; 2] = [Vec3::new(0.18, 0.55, 1.0), Vec3::new(1.0, 0.48, 0.07)];
const PPU: f32 = art::PIXELS_PER_UNIT;

fn frame_transform(f: Frame, z: f32) -> Transform {
    let t = f.tangent();
    Transform::from_xyz(f.centre[0], f.centre[1], z).with_rotation(Quat::from_rotation_z(t[1].atan2(t[0])))
}

/// The part of `mesh` (drawn with `tf`) in front of `f`'s surface.
pub(crate) fn clip_mesh(mesh: &Mesh, tf: &Transform, f: Frame) -> Option<Mesh> {
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
    let m = tf.compute_affine();
    let side: Vec<f32> = pos
        .iter()
        .map(|p| {
            let w = m.transform_point3(Vec3::from(*p));
            (w.x - f.centre[0]) * f.normal[0] + (w.y - f.centre[1]) * f.normal[1]
        })
        .collect();
    let index: Vec<usize> = match mesh.indices() {
        Some(i) => i.iter().collect(),
        None => (0..pos.len()).collect(),
    };
    let mut blends: Vec<(usize, usize, f32)> = Vec::new();
    for t in index.chunks_exact(3) {
        for tri in clip_triangle([side[t[0]], side[t[1]], side[t[2]]]) {
            blends.extend(tri.map(|(a, b, w)| (t[a], t[b], w)));
        }
    }
    if blends.is_empty() {
        blends = vec![(0, 0, 0.0); 3];
    }
    fn mix<const N: usize>(v: &[[f32; N]], blends: &[(usize, usize, f32)]) -> Vec<[f32; N]> {
        blends.iter().map(|&(a, b, w)| std::array::from_fn(|k| v[a][k] + (v[b][k] - v[a][k]) * w)).collect()
    }
    let mut out = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    for (attribute, values) in mesh.attributes() {
        let values = match values {
            VertexAttributeValues::Float32x2(v) => VertexAttributeValues::Float32x2(mix(v, &blends)),
            VertexAttributeValues::Float32x3(v) => VertexAttributeValues::Float32x3(mix(v, &blends)),
            VertexAttributeValues::Float32x4(v) => VertexAttributeValues::Float32x4(mix(v, &blends)),
            _ => return None,
        };
        out.insert_attribute(attribute.clone(), values);
    }
    Some(out)
}

/// The game's light sprite texture (HDParticle, 256 px: ModLightPrefab's), or a plain white one without the data.
fn game_light(play: &Play, images: &mut Assets<Image>, textures: &mut Textures) -> Handle<Image> {
    let found = play.game.assets.as_deref().and_then(|db| {
        let r = db.sprite_by_name("HDParticle")?;
        let tex = db.resolve(r.file, db.sprite(r).ok()?.texture)?;
        crate::play::load_texture(db, images, textures, tex)
    });
    found.map_or_else(|| images.add(Image::default()), |(h, _)| h)
}

/// After play::draw: the guns, the portals, the shots' beams and flashes.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut commands: Commands,
    mut play: ResMut<Play>,
    time: Res<Time>,
    mut images: ResMut<Assets<Image>>,
    mut textures: ResMut<Textures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut glows: ResMut<Assets<crate::lights::Glow>>,
    mut view: Local<Option<ViewArt>>,
    objects: Query<(&ObjectSprite, &Transform), (Without<GunSprite>, Without<Twin>)>,
    mut guns: Query<(&GunSprite, &mut Transform, &mut MeshMaterial2d<crate::lights::Glow>, &mut Visibility), (Without<ObjectSprite>, Without<PortalLine>, Without<Flash>)>,
    mut lines: Query<(&PortalLine, &mut Transform, &mut Visibility), (Without<GunSprite>, Without<ObjectSprite>, Without<Flash>, Without<Twin>, Without<LimbSprite>)>,
    mut flashes: Query<(Entity, &mut Flash, &mut Transform, &MeshMaterial2d<crate::lights::Glow>), (Without<GunSprite>, Without<ObjectSprite>, Without<PortalLine>, Without<Twin>, Without<LimbSprite>)>,
) {
    if view.is_none() {
        let soft = game_light(&play, &mut images, &mut textures);
        let mut image = |b: &[u8]| images.add(png_image(b));
        let line_tex = [art::LINE_BLUE, art::LINE_ORANGE].map(&mut image);
        let core_tex = image(art::GUN_CORE_GLOW);
        *view = Some(ViewArt {
            gun_mesh: meshes.add(Rectangle::new(art::GUN_SIZE[0] / PPU, art::GUN_SIZE[1] / PPU)),
            core: [0, 1].map(|i| glows.add(crate::lights::Glow { color: (COLOURS[i] * 1.6 + Vec3::splat(0.25)).extend(1.0), texture: core_tex.clone() })),
            line_mesh: meshes.add(Rectangle::new(2.0 * HALF_LEN, art::LINE_H / PPU)),
            quad: meshes.add(Rectangle::new(1.0, 1.0)),
            line: line_tex.clone().map(|t| glows.add(crate::lights::Glow { color: Vec4::splat(1.6), texture: t })),
            light: [0, 1].map(|i| glows.add(crate::lights::Glow { color: COLOURS[i].extend(1.0) * 1.1, texture: soft.clone() })),
            line_tex,
            soft,
        });
    }
    let v = view.as_ref().unwrap();
    let g = &mut play.game;
    let dt = time.delta_secs();
    let now = time.elapsed_secs();

    // The guns (their sprite is the renderer's): the core's glow and two lights, all on the gun's pose.
    let have = guns.iter().filter(|(s, ..)| s.1 == 1).count();
    for i in have..g.extras.guns.len() {
        commands.spawn((GunSprite(i, 1), Mesh2d(v.gun_mesh.clone()), MeshMaterial2d(v.core[0].clone()), Transform::default(), Visibility::Hidden));
        for part in [2, 3] {
            commands.spawn((GunSprite(i, part), Mesh2d(v.quad.clone()), MeshMaterial2d(v.light[0].clone()), Transform::default(), Visibility::Hidden, RenderLayers::layer(1)));
        }
    }
    let gun_pose = |i: usize| {
        let gun = &g.extras.guns[i];
        let p = &g.physicals[gun.p];
        let z = objects.iter().find(|(o, _)| o.0 == gun.p).map_or(0.0, |(_, t)| t.translation.z);
        let [x, y, _] = p.transform.position;
        let [qx, qy, qz, qw] = p.transform.rotation;
        let s = p.transform.localScale;
        let tf = Transform::from_xyz(x, y, z).with_rotation(Quat::from_xyzw(qx, qy, qz, qw)).with_scale(Vec3::new(s[0].signum(), s[1].signum(), 1.0));
        let shown = p.active && !p.destroyed && !p.isDisintegrated;
        (tf, shown, gun.colour)
    };
    for (&GunSprite(i, part), mut tf, mut material, mut visibility) in &mut guns {
        let (pose, shown, colour) = gun_pose(i);
        let Some(c) = colour else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let pulse = 1.0 + 0.12 * (now * 4.0).sin();
        *tf = match part {
            1 => pose.with_translation(pose.translation + Vec3::Z * 0.0005),
            // The lights: at the core and the top light (pixels from the sprite's centre), the gun's turn.
            _ => {
                let [px, py] = if part == 2 { art::GUN_CORE } else { art::GUN_LIGHT };
                let at = pose.transform_point(Vec3::new(px / PPU, py / PPU, 0.0));
                let size = if part == 2 { 1.1 } else { 0.5 } * pulse;
                Transform::from_translation(at.with_z(0.0)).with_scale(Vec3::new(size, size, 1.0))
            }
        };
        material.0 = if part == 1 { v.core[c].clone() } else { v.light[c].clone() };
        *visibility = if shown { Visibility::Inherited } else { Visibility::Hidden };
    }

    // The portals: the oval and its light, each as long as the opening.
    if lines.is_empty() {
        for i in 0..2 {
            commands.spawn((PortalLine(i, 0), Mesh2d(v.line_mesh.clone()), MeshMaterial2d(v.line[i].clone()), Transform::default(), Visibility::Hidden));
            commands.spawn((PortalLine(i, 1), Mesh2d(v.quad.clone()), MeshMaterial2d(v.light[i].clone()), Transform::default(), Visibility::Hidden, RenderLayers::layer(1)));
        }
    }
    for (line, mut tf, mut visibility) in &mut lines {
        let Some(pt) = g.extras.portals[line.0] else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let pulse = 1.0 + 0.06 * (now * 5.0 + line.0 as f32 * 2.0).sin();
        let along = pt.half / HALF_LEN;
        let base = frame_transform(pt.frame, 0.6);
        *tf = match line.1 {
            0 => base.with_scale(Vec3::new(along, 1.0, 1.0)),
            // The light: a soft oval reaching a little out in front of the surface.
            _ => {
                let n = Vec2::from(pt.frame.normal) * 0.3;
                base.with_translation(Vec3::new(pt.frame.centre[0] + n.x, pt.frame.centre[1] + n.y, 0.0)).with_scale(Vec3::new((2.0 * pt.half + 1.2) * pulse, 1.1 * pulse, 1.0))
            }
        };
        *visibility = Visibility::Inherited;
    }
    // Settings -> Mods "Portal glow": the lines' core, their lights and the gun's glows, live.
    let k = g.prefs.PortalGlow.clamp(0.25, 4.0);
    for i in 0..2 {
        let pulse = 1.0 + 0.15 * (now * 5.0 + i as f32 * 2.0).sin();
        if let Some(mut m) = glows.get_mut(&v.line[i]) {
            m.color = Vec4::splat(1.6 * pulse * k);
        }
        if let Some(mut m) = glows.get_mut(&v.light[i]) {
            m.color = COLOURS[i].extend(1.0) * 1.1 * k;
        }
        if let Some(mut m) = glows.get_mut(&v.core[i]) {
            m.color = ((COLOURS[i] * 1.6 + Vec3::splat(0.25)) * k).extend(1.0);
        }
    }

    // The moments: a fading beam along each shot, and flashes of light where it left the barrel, where a portal
    // opened or closed and where a shot fizzled.
    let mut flash = |commands: &mut Commands, glows: &mut Assets<crate::lights::Glow>, tex: &Handle<Image>, tf: Transform, colour: Vec4, life: f32, grow: f32, layer: usize| {
        let m = glows.add(crate::lights::Glow { color: colour, texture: tex.clone() });
        commands.spawn((Flash { life, max: life, colour, grow }, Mesh2d(v.quad.clone()), MeshMaterial2d(m), tf, RenderLayers::layer(layer)));
    };
    for fx in std::mem::take(&mut g.extras.fx) {
        match fx {
            Fx::Fire { colour, from, to } => {
                let (a, b) = (Vec2::from(from), Vec2::from(to));
                let d = b - a;
                let mid = (a + b) / 2.0;
                let tf = Transform::from_translation(mid.extend(0.61)).with_rotation(Quat::from_rotation_z(d.y.atan2(d.x))).with_scale(Vec3::new(d.length(), 5.0 / PPU, 1.0));
                flash(&mut commands, &mut glows, &v.line_tex[colour], tf, Vec4::splat(2.2), 0.18, 0.0, 0);
                flash(&mut commands, &mut glows, &v.soft, tf.with_translation(mid.extend(0.0)).with_scale(Vec3::new(d.length() + 1.0, 1.2, 1.0)), COLOURS[colour].extend(1.0) * 0.9, 0.18, 0.0, 1);
                flash(&mut commands, &mut glows, &v.soft, Transform::from_translation(a.extend(0.0)).with_scale(Vec3::splat(2.2)), COLOURS[colour].extend(1.0) * 2.0, 0.15, 0.5, 1);
            }
            Fx::Open { colour, frame } | Fx::Close { colour, frame } => {
                let open = matches!(fx, Fx::Open { .. });
                let half = g.extras.portals[colour].filter(|_| open).map_or(HALF_LEN, |p| p.half);
                let tf = frame_transform(frame, 0.0).with_scale(Vec3::new(2.0 * half + 2.0, 2.5, 1.0));
                flash(&mut commands, &mut glows, &v.soft, tf, COLOURS[colour].extend(1.0) * if open { 2.5 } else { 1.6 }, if open { 0.45 } else { 0.3 }, if open { 0.3 } else { -0.6 }, 1);
                let haze = frame_transform(frame, 0.61).with_scale(Vec3::new(2.0 * half + 0.6, 0.5, 1.0));
                flash(&mut commands, &mut glows, &v.soft, haze, COLOURS[colour].extend(1.0) * 1.2, if open { 0.35 } else { 0.25 }, if open { 0.0 } else { -0.9 }, 0);
            }
            Fx::Fizzle { colour, at, .. } => {
                let tf = Transform::from_translation(Vec2::from(at).extend(0.0)).with_scale(Vec3::splat(2.0));
                flash(&mut commands, &mut glows, &v.soft, tf, COLOURS[colour].extend(1.0) * 1.8, 0.3, 0.4, 1);
            }
        }
    }
    for (e, mut f, mut tf, material) in &mut flashes {
        f.life -= dt;
        if f.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let k = f.life / f.max;
        if let Some(mut m) = glows.get_mut(&material.0) {
            m.color = f.colour * k * k;
        }
        let s = 1.0 + f.grow * dt / f.max;
        tf.scale.x *= s;
        tf.scale.y *= s;
    }
}

/// After `draw`: the doorway. What is in one is clipped at its surface, and its twin is drawn at the other portal.
pub fn doorway(
    mut commands: Commands,
    play: Res<Play>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut drawn: Query<(Entity, &mut Mesh2d, &Transform, Option<&ObjectSprite>, Option<&LimbSprite>, Option<&GunSprite>, Option<&RendererSprite>, Option<&Clipped>), (Without<Twin>, Without<PortalLine>, Without<Flash>)>,
) {
    let g = &play.game;
    for (e, mut mesh, tf, object, limb, gun, renderer, clipped) in &mut drawn {
        let p = match (object, limb, gun, renderer) {
            (Some(o), ..) => o.0,
            (_, Some(l), ..) => g.limbs[l.0].PhysicalBehaviour,
            (_, _, Some(&GunSprite(i, 1)), _) => g.extras.guns[i].p,
            (.., Some(r)) => match g.renderers[r.0].anchor {
                crate::game::BareAnchor::Physical(p) => p,
                _ => continue,
            },
            _ => continue,
        };
        let orig = match clipped {
            Some(c) if mesh.0 == c.shown => c.orig.clone(),
            _ => mesh.0.clone(),
        };
        let pair = g.physicals.get(p).filter(|ph| !ph.destroyed).and_then(|ph| g.extras_doorway(ph.rigidbody.body));
        let Some(pair) = pair else {
            if let Some(c) = clipped {
                commands.entity(c.twin).despawn();
                mesh.0 = orig;
                commands.entity(e).remove::<Clipped>();
            }
            continue;
        };
        let twin_tf = Transform {
            translation: Vec2::from(pair.map_point([tf.translation.x, tf.translation.y])).extend(tf.translation.z),
            rotation: Quat::from_rotation_z(pair.rotation) * tf.rotation,
            scale: tf.scale,
        };
        let Some(source) = meshes.get(&orig) else { continue };
        let (Some(front), Some(back)) = (clip_mesh(source, tf, pair.entry), clip_mesh(source, &twin_tf, pair.exit)) else { continue };
        if let Some(c) = clipped {
            // The same twin: its look follows the sprite's (material, colour, order), its mesh and place are its own.
            let (twin, shown, twin_mesh) = (c.twin, c.shown.clone(), c.twin_mesh.clone());
            if let Some(mut m) = meshes.get_mut(&shown) {
                *m = front;
            }
            if let Some(mut m) = meshes.get_mut(&twin_mesh) {
                *m = back;
            }
            mesh.0 = shown;
            commands.entity(e).clone_with_opt_out(twin, |b| {
                b.deny::<(Clipped, Mesh2d, Transform)>();
            });
            commands.entity(twin).insert(twin_tf);
            continue;
        }
        let shown = meshes.add(front);
        let twin_mesh = meshes.add(back);
        mesh.0 = shown.clone();
        let twin = commands
            .entity(e)
            .clone_and_spawn_with_opt_out(|b| {
                b.deny::<Clipped>();
            })
            .insert((Mesh2d(twin_mesh.clone()), twin_tf, Twin))
            .id();
        commands.entity(e).insert(Clipped { orig, shown, twin, twin_mesh });
    }
}

/// PP_PORTAL=<scene> (with PPG_SHOT and --play-map Default, the portal check): a gun flipped on the floor fires an
/// orange portal into the left wall, a blue one opens on the floor, then the scene: crate (one rests on the floor portal and
/// sinks through), human (one stands on it), fast (a crate dropped from high), moving (the blue portal on a tumbling
/// plank with a crate on it), fling / flinghuman (a crate / human thrown down into it at 120 m/s), dangle (a human hung by the head with its body
/// through it; PP_DANGLE = how far lowered, default 2.2 m), probe / names (logs). PP_PORTAL_LOG=1 logs the bodies four times a second.
pub fn add_test(app: &mut App, scene: String) {
    use crate::game::Game;
    fn spawn(g: &mut Game, name: &str, at: [f32; 2], flipped: bool) -> usize {
        let mut db = g.assets.take().expect("the game's data");
        let first = g.physicals.len();
        let (inst, _) = g.catalog_spawn(&mut db, name, [at[0], at[1], 0.0], flipped).expect("PP_PORTAL: the spawn");
        g.assets = Some(db);
        g.unsynced.push(inst);
        first
    }
    let log = std::env::var("PP_PORTAL_LOG").is_ok();
    app.add_systems(Update, move |time: Res<Time>, play: Option<ResMut<Play>>, mut step: Local<u32>, mut first: Local<usize>, mut last_log: Local<f32>, mut aux: Local<usize>| {
        let Some(mut play) = play else { return };
        let g = &mut play.game;
        let t = time.elapsed_secs();
        match (*step, scene.as_str()) {
            (0, "probe" | "names") if t >= 1.0 => {
                *step = 9;
                let b = g.BoundingBox.clone();
                eprintln!("PORTAL bbox {:?} {:?}", b.center, b.extents);
                for (from, dir) in [([0.0, 5.0], [0.0, -1.0]), ([0.0, 1.0], [-1.0, 0.0]), ([0.0, 1.0], [1.0, 0.0]), ([0.0, 1.0], [0.0, 1.0])] {
                    let hit = g.raycast_first(crate::b2::math::Vec2::new(from[0], from[1]), dir, 500.0);
                    eprintln!("PORTAL ray {from:?} {dir:?} {:?}", hit.map(|h| (h.point, h.normal, h.body)));
                }
                if scene == "names" {
                    let mut db = g.assets.take().expect("the game's data");
                    eprintln!("PORTAL names {:?}", Game::catalog_names(&mut db));
                    g.assets = Some(db);
                }
            }
            // held: a human holds the gun; it fires blue at 6.0 s and orange at 7.4 s (where the gun points).
            (0, "held") if t >= 4.0 => {
                *step = 20;
                *first = spawn(g, "Human", [2.0, 1.5], false);
            }
            (20, _) if t >= 5.0 => {
                *step = 21;
                let grip = g.grips.iter().rposition(|gr| gr.PhysicalBehaviour >= *first).expect("a hand");
                let [x, y, _] = g.physicals[g.grips[grip].PhysicalBehaviour].transform.position;
                spawn(g, ppg_extras::PORTAL_GUN, [x + 0.3, y], false);
            }
            (21, _) => {
                *step = 22;
                for grip in (0..g.grips.len()).filter(|&k| g.grips[k].PhysicalBehaviour >= *first).collect::<Vec<_>>() {
                    g.grip_use(grip);
                }
                eprintln!("PORTAL held {:?}", g.grips.iter().filter(|gr| gr.PhysicalBehaviour >= *first).map(|gr| gr.CurrentlyHolding).collect::<Vec<_>>());
            }
            (22, _) if t >= 6.0 => {
                *step = 23;
                g.extras_fire(0, 0);
                eprintln!("PORTAL fire blue {:?}", g.extras.portals[0].map(|p| p.frame));
            }
            (23, _) if t >= 7.4 => {
                *step = 24;
                g.extras_fire(0, 1);
                eprintln!("PORTAL fire orange {:?}", g.extras.portals[1].map(|p| p.frame));
            }
            // fxlab: candidate game prefabs side by side, blue (PP_FX_AGE: shot this long after they start).
            (0, "fxlab") if t >= 4.0 => {
                *step = 30;
                let list = ["Prefabs/Exotic flash", "Prefabs/PulseExplosion", "Prefabs/BrokenElectronicsSpark", "Prefabs/BigZap", "Prefabs/Ricochet", "Prefabs/RaygunExplosion", "Prefabs/SpawnParticles", "Prefabs/ChargedParticles", "Prefabs/HugeZap", "Prefabs/MuzzleSmoke"];
                let only: Option<usize> = std::env::var("PP_FX").ok().and_then(|v| v.parse().ok());
                for (k, path) in list.iter().enumerate().filter(|(k, _)| only.is_none_or(|o| o == *k)) {
                    let found = g.assets.as_deref().and_then(|db| db.resource(path, 1).ok().flatten()).is_some();
                    eprintln!("PORTAL fx {k} {path} found={found} x={}", -4.0 + k as f32 * 2.0);
                    g.extras_effect(path, if only.is_some() { [2.0, 1.0] } else { [-4.0 + k as f32 * 2.0, 1.0] }, 0.0, 1.0, 0);
                }
            }
            // gun: the gun on the floor fires blue at 4.3 s and orange at 5.3 s (for close-ups of its colours).
            (1, "gun" | "copy") if t >= 4.3 => {
                *step = 40;
                g.extras_fire(0, 0);
            }
            (40, _) if t >= 5.3 => {
                *step = 41;
                g.extras_fire(0, 1);
            }
            // copy: the gun is copied and pasted (ObjectState, the path saves take too); the copy fires blue at 6.6 s.
            (41, "copy") if t >= 5.8 => {
                *step = 42;
                let p = g.extras.guns[0].p;
                let states = g.object_states_of(&[p], [7.0, 0.4, 0.0]);
                eprintln!("PORTAL copy {:?}", states.iter().map(|s| s.SpawnableAssetName.clone()).collect::<Vec<_>>());
                g.paste_states(&states, [3.0, 1.0, 0.0], false, "Paste");
            }
            (42, _) if t >= 6.6 => {
                *step = 43;
                let names: Vec<_> = g.extras.guns.iter().map(|gun| g.physicals[gun.p].name.clone()).collect();
                eprintln!("PORTAL guns {names:?}");
                g.extras_fire(g.extras.guns.len() - 1, 0);
                eprintln!("PORTAL copy fired blue {:?}", g.extras.portals[0].map(|p| p.frame));
            }
            // resize: at 0.6x the gun fires orange again; both portals are then 0.6x (the blue one resized in place).
            (3, "resize") if t >= 5.5 => {
                *step = 44;
                g.prefs.PortalSize = 0.6;
                g.extras_fire(0, 1);
                eprintln!("PORTAL resize halves {:?}", g.extras.portals.map(|p| p.map(|p| p.half)));
            }
            // shot: a Pistol above the floor portal, held pointing down, fires into it at 5.8 s: the bullet comes out of the
            // wall portal into a Human standing in front of it.
            (3, "shot") if t >= 5.4 => {
                *step = 50;
                let body = g.physicals[*aux].rigidbody.body;
                g.world.set_transform(body, crate::b2::math::Vec2::new(3.0, 2.2), -std::f32::consts::FRAC_PI_2);
                let b = g.world.body_mut(body);
                (b.linear_velocity, b.angular_velocity, b.gravity_scale) = (crate::b2::math::Vec2::ZERO, 0.0, 0.0);
            }
            (50, _) if t >= 5.8 => {
                *step = 51;
                let health: f32 = g.limbs.iter().filter(|l| l.PhysicalBehaviour >= *first).map(|l| l.Health).sum();
                eprintln!("PORTAL shot: human health before {health:.1}");
                g.activate_for_test(*aux);
            }
            (51, _) if t >= 6.4 => {
                *step = 52;
                let health: f32 = g.limbs.iter().filter(|l| l.PhysicalBehaviour >= *first).map(|l| l.Health).sum();
                eprintln!("PORTAL shot: human health after {health:.1}");
            }
            (0, _) if t >= 4.0 => {
                *step = 1;
                spawn(g, ppg_extras::PORTAL_GUN, [7.0, 0.4], true);
            }
            (1, _) if t >= 4.3 => {
                // The gun fires orange into the left wall, then the scene's things are spawned (before the floor portal opens,
                // so the spawn outline has faded when they go in).
                *step = 2;
                g.extras_fire(0, 1);
                *first = match scene.as_str() {
                    "crate" => spawn(g, "Crate", [3.0, 0.8], false),
                    "human" => spawn(g, "Human", [3.0, 2.6], false),
                    "fast" => spawn(g, "Crate", [3.0, 14.0], false),
                    "fling" => spawn(g, "Crate", [3.0, 6.0], false),
                    "flinghuman" => spawn(g, "Human", [3.0, 6.0], false),
                    "dangle" => spawn(g, "Human", [3.0, 2.6], false),
                    "moving" => spawn(g, "Plank", [3.0, 0.5], false),
                    "shot" => spawn(g, "Human", [-3.5, 2.6], false),
                    "resize" => spawn(g, "Brick", [0.0, 0.5], false),
                    _ => *first,
                };
            }
            (2, _) if t >= 4.9 => {
                *step = 3;
                // The shot passes the scene's things (as if fired before they came), but lands on the moving scene's plank.
                // (The moving scene's plank is shorter than a default portal: it uses the Portal size setting at its least, 0.5x.)
                if scene == "moving" {
                    g.prefs.PortalSize = 0.5;
                }
                let skip: Vec<_> = if scene == "moving" { vec![] } else { g.physicals[*first..].iter().map(|p| p.rigidbody.body).collect() };
                let opened = g.extras_place(0, [3.0, 6.0], [0.0, -1.0], &skip);
                eprintln!("PORTAL orange {:?} blue {opened} {:?}", g.extras.portals[1].map(|p| p.frame), g.extras.portals[0].map(|p| p.frame));
                if scene == "moving" {
                    let plank = g.physicals.iter().position(|p| p.name.contains("Plank")).expect("the plank");
                    let body = g.world.body_mut(g.physicals[plank].rigidbody.body);
                    body.set_sleeping_false();
                    body.set_linear_velocity(crate::b2::math::Vec2::new(-1.0, 7.0));
                    body.angular_velocity = 1.2;
                    spawn(g, "Crate", [2.0, 4.0], false);
                }
                if scene == "shot" {
                    *aux = spawn(g, "Pistol", [3.0, 2.2], false);
                }
                // dangle: the human lowered into the floor portal to the chest and hung by the head (held still, as by a hand).
                if scene == "dangle" {
                    let drop = std::env::var("PP_DANGLE").ok().and_then(|v| v.parse().ok()).unwrap_or(2.2);
                    for p in *first..g.physicals.len() {
                        let b = g.physicals[p].rigidbody.body;
                        let (o, a) = (g.world.body(b).xf.p, g.world.body(b).sweep.a);
                        g.world.set_transform(b, o - crate::b2::math::Vec2::new(0.0, drop), a);
                        if g.physicals[p].name.contains("Head") {
                            g.world.set_body_type(b, crate::b2::world::BodyType::Static);
                        }
                    }
                }
                // fling / flinghuman: thrown down into the floor portal at 120 m/s (2 m a step; PP_FLING_SPEED sets it).
                if scene.starts_with("fling") {
                    for p in *first..g.physicals.len() {
                        let body = g.world.body_mut(g.physicals[p].rigidbody.body);
                        body.set_sleeping_false();
                        body.set_linear_velocity(crate::b2::math::Vec2::new(0.0, -std::env::var("PP_FLING_SPEED").ok().and_then(|v| v.parse().ok()).unwrap_or(120.0)));
                    }
                }
            }
            _ => {}
        }
        if log && *first > 0 && *step >= 2 && t - *last_log >= 0.25 {
            *last_log = t;
            let mut line = format!("PORTAL t={t:.2} crossers={} portals={:?}", g.extras.crossers.len(), g.extras.portals.map(|p| p.map(|p| (p.frame.centre, p.body))));
            for &(l, _) in &g.tracer_pool.held {
                let x = &g.lines[l];
                line += &format!(" | tracer at {:?} q {:?} to {:?} w {} on {}", x.chain[0].t, x.chain[0].q, x.positions.get(1), x.width, x.enabled && x.visible);
            }
            for p in (*first..g.physicals.len()).take(16) {
                let ph = &g.physicals[p];
                if ph.destroyed {
                    continue;
                }
                let b = g.world.body(ph.rigidbody.body);
                line += &format!(" | {} ({:.2},{:.2}) v=({:.1},{:.1}){}", ph.name, b.sweep.c.x, b.sweep.c.y, b.linear_velocity.x, b.linear_velocity.y, if g.extras.crossers.contains_key(&ph.rigidbody.body) { " IN" } else { "" });
            }
            eprintln!("{line}");
        }
    });
}
