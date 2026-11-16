//! DecalControllerBehaviour's SpriteRenderers: each decal and drip streak is a quad in its object's local space, visible
//! inside the SpriteMask of the object's sprite (the mask ignores the sprite's flip and draw mode, as SpriteMask does).
use crate::assets::components::Sprite as UnitySprite;
use crate::assets::db::{AssetDb, ObjRef};
use crate::game::{misc::Streak, Game};
use crate::play::{load_texture, sprite_line_key, Play, Textures};
use bevy::{
    asset::{embedded_asset, RenderAssetUsages},
    mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError},
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPipeline, Material2dPlugin},
};
use std::collections::HashMap;

/// SpriteMask.alphaCutoff's default.
const MASK_CUTOFF: f32 = 0.2;

#[derive(ShaderType, Clone, PartialEq)]
pub struct MaskSettings {
    /// The object's sprite texture coordinates from world x, y, 1.
    u: Vec4,
    v: Vec4,
    /// The sprite's region of its texture: min u, min v, max u, max v.
    rect: Vec4,
    cutoff: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct DecalMaterial {
    #[uniform(0)]
    settings: MaskSettings,
    #[texture(1)]
    #[sampler(2)]
    texture: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    mask: Handle<Image>,
}

impl Material2d for DecalMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/decal.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    // A rotated object mirrors its decals (InverseTransformVector of the size).
    fn specialize(_: &Material2dPipeline, descriptor: &mut RenderPipelineDescriptor, _: &MeshVertexBufferLayoutRef, _: Material2dKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

pub fn add(app: &mut App) {
    embedded_asset!(app, "decal.wesl");
    app.add_plugins(Material2dPlugin::<DecalMaterial>::default());
}

/// A decal's or streak's entity: object, controller, decal, streak.
#[derive(Component)]
#[require(crate::play::MainScene)]
pub struct DecalSprite {
    physical: usize,
    controller: usize,
    decal: usize,
    streak: Option<usize>,
    /// The decal's local position, to notice a Clear and a new decal in its place.
    position: [f32; 2],
    /// The streak's transform the mesh was built with.
    built: Option<(f32, [f32; 2])>,
    material: AssetId<DecalMaterial>,
}

/// A sprite's rect corners about its pivot, and its region of the texture.
struct Quad {
    corners: [[f32; 2]; 4],
    uv: [[f32; 2]; 4],
}

fn quad(s: &UnitySprite, texture: [f32; 2]) -> Quad {
    let [x, y, w, h] = s.texture_rect;
    let at = |k: [f32; 2]| [(s.texture_rect_offset[0] + k[0] * w - s.pivot[0] * s.rect[2]) / s.pixels_to_units, (s.texture_rect_offset[1] + k[1] * h - s.pivot[1] * s.rect[3]) / s.pixels_to_units];
    let uv = |k: [f32; 2]| [(x + k[0] * w) / texture[0], 1.0 - (y + k[1] * h) / texture[1]];
    let k = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    Quad { corners: k.map(at), uv: k.map(uv) }
}

fn quad_mesh(q: &Quad, place: impl Fn([f32; 2]) -> [f32; 2], color: [f32; 4]) -> Mesh {
    let c = Color::srgba(color[0], color[1], color[2], color[3]).to_linear().to_f32_array();
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, q.corners.map(|p| place(p)).map(|p| [p[0], p[1], 0.0]).to_vec());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, q.uv.to_vec());
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; 4]);
    mesh.insert_indices(Indices::U16(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

/// The object's sprite's Transform (as `play::draw` places it).
fn object_transform(g: &Game, p: usize, person: Option<usize>) -> Transform {
    let Some(person) = person else { return crate::play::object_sprite_transform(g, p, 0.0, 1.0) };
    let ph = &g.physicals[p];
    let [x, y, _] = ph.transform.position;
    let [qx, qy, qz, qw] = ph.transform.rotation;
    let s = ph.transform.localScale;
    let [sx, sy] = [g.people[person].localScale[0], g.people[person].localScale[1]];
    Transform::from_xyz(x, y, 0.0).with_rotation(Quat::from_xyzw(qx, qy, qz, qw)).with_scale(Vec3::new(sx * s[0], sy * s[1], 1.0))
}

fn mask_settings(mask: &UnitySprite, texture: [f32; 2], t: &Transform) -> MaskSettings {
    // World to the object's local: the inverse of translate, rotate, scale.
    let (c, s) = {
        let r = t.rotation * Vec3::X;
        (r.x, r.y)
    };
    let (tx, ty) = (t.translation.x, t.translation.y);
    let lx = [c / t.scale.x, s / t.scale.x, -(tx * c + ty * s) / t.scale.x];
    let ly = [-s / t.scale.y, c / t.scale.y, (tx * s - ty * c) / t.scale.y];
    let [x, y, w, h] = mask.texture_rect;
    // Local to texture: x = (local * ppu + pivot - offset + rect corner) / size.
    let ox = x + mask.pivot[0] * mask.rect[2] - mask.texture_rect_offset[0];
    let oy = y + mask.pivot[1] * mask.rect[3] - mask.texture_rect_offset[1];
    let k = mask.pixels_to_units;
    let u = Vec4::new(lx[0] * k / texture[0], lx[1] * k / texture[0], (lx[2] * k + ox) / texture[0], 0.0);
    let v = Vec4::new(-ly[0] * k / texture[1], -ly[1] * k / texture[1], 1.0 - (ly[2] * k + oy) / texture[1], 0.0);
    let rect = Vec4::new(x / texture[0], 1.0 - (y + h) / texture[1], (x + w) / texture[0], 1.0 - y / texture[1]);
    MaskSettings { u, v, rect, cutoff: MASK_CUTOFF }
}

/// The sprite, read once: an atlased one is found by parsing its whole atlas, too slow for every decal every frame.
fn sprite(db: &AssetDb, sprites: &mut HashMap<ObjRef, UnitySprite>, r: ObjRef) -> UnitySprite {
    sprites.entry(r).or_insert_with(|| db.sprite(r).unwrap()).clone()
}

/// A streak in its decal's space.
fn streak_place(streak: &Streak) -> impl Fn([f32; 2]) -> [f32; 2] + use<> {
    let (position, scale, z) = (streak.position, streak.scale, streak.z.to_radians());
    let (s, c) = (crate::unity::sinf(z), crate::unity::cosf(z));
    move |p| {
        let q = [p[0] * scale[0], p[1] * scale[1]];
        [q[0] * c - q[1] * s + position[0], q[0] * s + q[1] * c + position[1]]
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn decals(
    mut commands: Commands,
    play: Res<Play>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<DecalMaterial>>,
    mut textures: ResMut<Textures>,
    mut shared: Local<HashMap<(usize, usize, AssetId<Image>), Handle<DecalMaterial>>>,
    mut placed: Query<(Entity, &mut DecalSprite, &Mesh2d, &mut Transform, &mut Visibility)>,
    mut parsed: Local<HashMap<ObjRef, UnitySprite>>,
) {
    // The masks' materials are the old objects' (by index).
    if play.is_added() {
        shared.clear();
    }
    let g = &play.game;
    let Some(db) = g.assets.as_deref() else { return };
    let person = |p: usize| g.limbs.iter().find(|l| l.PhysicalBehaviour == p).map(|l| l.Person);
    let mut have = std::collections::HashSet::new();
    let mut moved: HashMap<usize, Transform> = HashMap::new();
    for (entity, mut d, mesh, mut transform, mut visibility) in &mut placed {
        let decal = g.physicals[d.physical].decal_controllers.get(d.controller).and_then(|c| c.decals.get(d.decal)).filter(|x| x.position == d.position);
        let Some(decal) = decal else {
            commands.entity(entity).despawn();
            continue;
        };
        have.insert((d.physical, d.controller, d.decal, d.streak));
        let t = *moved.entry(d.physical).or_insert_with(|| object_transform(g, d.physical, person(d.physical)));
        (transform.translation.x, transform.translation.y) = (t.translation.x, t.translation.y);
        transform.rotation = t.rotation;
        transform.scale = t.scale;
        let ph = &g.physicals[d.physical];
        *visibility = if ph.active && !ph.isDisintegrated { Visibility::Inherited } else { Visibility::Hidden };
        if let Some(streak) = d.streak.and_then(|i| decal.streaks.get(i)) {
            if d.built != Some((streak.z, streak.scale)) {
                let controller = &ph.decal_controllers[d.controller];
                let r = g.decal_sprites[&controller.descriptor].1.unwrap();
                let s = sprite(db, &mut parsed, r);
                let (_, size) = load_texture(db, &mut images, &mut textures, db.resolve(r.file, s.texture).unwrap()).unwrap();
                let place = streak_place(streak);
                let (at, scale) = (decal.position, decal.scale);
                let _ = meshes.insert(mesh.id(), quad_mesh(&quad(&s, size), |p| {
                    let q = place(p);
                    [q[0] * scale[0] + at[0], q[1] * scale[1] + at[1]]
                }, decal.colour));
                d.built = Some((streak.z, streak.scale));
            }
        }
    }
    // The mask follows the object.
    for ((p, c), handle) in shared.iter().map(|((p, c, _), h)| ((p, c), h)) {
        if let Some(t) = moved.get(p) {
            if let Some(m) = materials.get(handle.id()) {
                let Some((Some(r), _, _)) = g.physicals[*p].decal_controllers.get(*c).and_then(|c| c.holder) else { continue };
                let (mask, texture) = mask_of(db, &mut parsed, r, &mut images, &mut textures);
                let settings = mask_settings(&mask, texture, t);
                // A Clear and a new CreateContainer may take another sprite for the mask.
                let (mask_image, _) = load_texture(db, &mut images, &mut textures, db.resolve(r.file, mask.texture).unwrap()).unwrap();
                if m.settings != settings || m.mask != mask_image {
                    let mut m = materials.get_mut(handle.id()).unwrap();
                    (m.settings, m.mask) = (settings, mask_image);
                }
            }
        }
    }
    for (p, ph) in g.physicals.iter().enumerate() {
        for (c, controller) in ph.decal_controllers.iter().enumerate() {
            for (i, decal) in controller.decals.iter().enumerate() {
                let Some(&(ref sprites, drip)) = g.decal_sprites.get(&controller.descriptor) else { continue };
                let wanted = std::iter::once(None).chain((0..decal.streaks.len()).map(Some));
                for streak in wanted {
                    if have.contains(&(p, c, i, streak)) {
                        continue;
                    }
                    let r = if streak.is_some() { drip.unwrap() } else { sprites[decal.sprite] };
                    let s = sprite(db, &mut parsed, r);
                    let (image, size) = load_texture(db, &mut images, &mut textures, db.resolve(r.file, s.texture).unwrap()).unwrap();
                    let (at, scale) = (decal.position, decal.scale);
                    let q = quad(&s, size);
                    let mesh = match streak {
                        None => quad_mesh(&q, |p| [p[0] * scale[0] + at[0], p[1] * scale[1] + at[1]], decal.colour),
                        Some(k) => {
                            let place = streak_place(&decal.streaks[k]);
                            quad_mesh(&q, |p| {
                                let q = place(p);
                                [q[0] * scale[0] + at[0], q[1] * scale[1] + at[1]]
                            }, decal.colour)
                        }
                    };
                    // The decalHolder's SpriteMask and SortingGroup, from CreateContainer.
                    let (mask_sprite, layer, order) = controller.holder.expect("a decal without its decalHolder");
                    let mask_sprite = mask_sprite.expect("a drawn object keeps its SpriteRenderer sprite");
                    let (mask, texture) = mask_of(db, &mut parsed, mask_sprite, &mut images, &mut textures);
                    let (mask_image, _) = load_texture(db, &mut images, &mut textures, db.resolve(mask_sprite.file, mask.texture).unwrap()).unwrap();
                    let t = object_transform(g, p, person(p));
                    let handle = shared.entry((p, c, image.id())).or_insert_with(|| materials.add(DecalMaterial { settings: mask_settings(&mask, texture, &t), texture: image, mask: mask_image })).clone();
                    let order = order + 1;
                    commands.spawn((
                        DecalSprite { physical: p, controller: c, decal: i, streak, position: at, built: None, material: handle.id() },
                        Mesh2d(meshes.add(mesh)),
                        MeshMaterial2d(handle),
                        t,
                        sprite_line_key(layer, order, 3000, p),
                    ));
                }
            }
        }
    }
}

fn mask_of(db: &AssetDb, sprites: &mut HashMap<ObjRef, UnitySprite>, r: ObjRef, images: &mut Assets<Image>, textures: &mut Textures) -> (UnitySprite, [f32; 2]) {
    let s = sprite(db, sprites, r);
    let (_, size) = load_texture(db, images, textures, db.resolve(r.file, s.texture).unwrap()).unwrap();
    (s, size)
}
