//! PhysicalBehaviour's selection outline: CreateOutlineObject's child SpriteRenderer (Simple, unflipped, the "Top"
//! sorting layer at int.MaxValue, Global.main.SelectionOutlineMaterial) showing the object's sprite, active while
//! ShowOutline is.
// Utils.UpdateOutlineMaterial: outline() below, the same sprite, _AtlasTransform (GetMin of the uvs, rect over texture size) and _MainTex as PhysicalBehaviour.RefreshOutline, without setting _ObjectScale; its only caller, MapEditorSelectionOutlineBehaviour, is in dead.tsv
use crate::play::{simple_sprite, Play, Textures};
use bevy::{
    asset::embedded_asset,
    mesh::MeshVertexBufferLayoutRef,
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError},
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPipeline, Material2dPlugin},
};

/// Material/SelectionOutline.mat and FrozenOutline.mat (same shader): _GlowColour (sRGB) and _Threshold.
const SELECTION: ([f32; 4], f32) = ([0.0, 1.0, 0.0, 0.72156864], 0.6);
const FROZEN: ([f32; 4], f32) = ([0.1435935, 0.24708432, 0.22767979, 1.0], 0.5);

#[derive(ShaderType, Clone)]
struct OutlineSettings {
    glow: Vec4,
    atlas: Vec4,
    texel: Vec4,
    object_scale: Vec2,
    threshold: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct OutlineMaterial {
    #[uniform(0)]
    settings: OutlineSettings,
    #[texture(1)]
    #[sampler(2)]
    texture: Handle<Image>,
}

impl Material2d for OutlineMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/outline.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    // Cull Off.
    fn specialize(_: &Material2dPipeline, descriptor: &mut RenderPipelineDescriptor, _: &MeshVertexBufferLayoutRef, _: Material2dKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// An outline: a physical object's selection outline, or a FreezeBehaviour's.
#[derive(Component, Clone, Copy)]
#[require(crate::play::MainScene)]
pub enum Outline {
    Selection(usize),
    Frozen(usize),
}

/// Above every ranked sprite (they sit in -0.5 to 0.5): the "Top" layer, the selection at int.MaxValue and the frozen
/// one at int.MaxValue - 1.
const SELECTION_Z: f32 = 0.75;
const FROZEN_Z: f32 = 0.74;

pub fn add(app: &mut App) {
    embedded_asset!(app, "outline.wesl");
    app.add_plugins(Material2dPlugin::<OutlineMaterial>::default());
    app.add_systems(PostUpdate, outlines.before(bevy::transform::TransformSystems::Propagate));
}

/// The outline of sprite `r`: its mesh and a material with _AtlasTransform from it (_ObjectScale one: the selection's;
/// FreezeBehaviour's zero divides nothing either).
fn outline(cache: &crate::game::limb_sprites::LimbSpriteCache, db: &crate::assets::db::AssetDb, images: &mut Assets<Image>, meshes: &mut Assets<Mesh>, materials: &mut Assets<OutlineMaterial>, textures: &mut Textures, r: crate::assets::db::ObjRef, (colour, threshold): ([f32; 4], f32)) -> Option<(Mesh2d, MeshMaterial2d<OutlineMaterial>)> {
    let (mesh, texture, size, atlas) = simple_sprite(cache, db, images, textures, r)?;
    let glow = Color::srgba(colour[0], colour[1], colour[2], colour[3]).to_linear().to_vec4();
    let settings = OutlineSettings { glow, atlas: Vec4::from_array(atlas), texel: Vec4::new(1.0 / size[0], 1.0 / size[1], size[0], size[1]), object_scale: Vec2::ONE, threshold };
    Some((Mesh2d(meshes.add(mesh)), MeshMaterial2d(materials.add(OutlineMaterial { settings, texture }))))
}

/// PhysicalBehaviour.RefreshOutline (from CreateOutlineObject in Awake) and FreezeBehaviour.CreateOutlineObject: the
/// outline takes the object's sprite as first seen; then each frame it follows the object, shown while ShowOutline (or
/// the FreezeBehaviour's `outline`) is.
#[allow(clippy::too_many_arguments)]
fn outlines(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<OutlineMaterial>>,
    play: Option<Res<Play>>,
    mut textures: ResMut<Textures>,
    mut made: Local<(Vec<bool>, Vec<bool>, usize)>,
    mut placed: Query<(&Outline, &mut Transform, &mut Visibility, &MeshMaterial2d<OutlineMaterial>)>,
) {
    let Some(play) = play else { return };
    let g = &play.game;
    let Some(db) = g.assets.as_deref() else { return };
    let (physicals, freezes, spawned) = &mut *made;
    // A map load despawns them (ui::loading): the new game's objects start over.
    if placed.iter().len() < *spawned {
        physicals.clear();
        freezes.clear();
        *spawned = 0;
    }
    physicals.resize(g.physicals.len(), false);
    freezes.resize(g.freezes.len(), false);
    for p in 0..g.physicals.len() {
        if std::mem::replace(&mut physicals[p], true) {
            continue;
        }
        let Some((r, ..)) = g.physicals[p].sprite else { continue };
        let Some(drawn) = outline(&g.limb_sprite_cache, db, &mut images, &mut meshes, &mut materials, &mut textures, r, SELECTION) else { continue };
        commands.spawn((Outline::Selection(p), drawn, Transform::from_xyz(0.0, 0.0, SELECTION_Z), Visibility::Hidden, crate::lights::camera_layers(16)));
        *spawned += 1;
    }
    // FreezeBehaviour.Start made one when ShowOutlines and HasOutline.
    for f in 0..g.freezes.len() {
        if freezes[f] || !g.freezes[f].outline {
            continue;
        }
        freezes[f] = true;
        let Some((r, ..)) = g.physicals[g.freezes[f].physical].sprite else { continue };
        let Some(drawn) = outline(&g.limb_sprite_cache, db, &mut images, &mut meshes, &mut materials, &mut textures, r, FROZEN) else { continue };
        commands.spawn((Outline::Frozen(f), drawn, Transform::from_xyz(0.0, 0.0, FROZEN_Z), Visibility::Hidden, crate::lights::camera_layers(16)));
        *spawned += 1;
    }
    for (&o, mut transform, mut visibility, material) in &mut placed {
        // ResizableSliceBehaviour.OnWillRenderObject's _ObjectScale on the selection outline (sprite_blocks.rs).
        if let Outline::Selection(p) = o {
            if let Some([x, y]) = g.resizable_slices.iter().find(|r| r.physical == p).and_then(|r| r.outline_scale) {
                if materials.get(&material.0).is_some_and(|m| m.settings.object_scale != Vec2::new(x, y)) {
                    materials.get_mut(&material.0).unwrap().settings.object_scale = Vec2::new(x, y);
                }
            }
        }
        let (p, z, on) = match o {
            Outline::Selection(p) => (p, SELECTION_Z, g.physicals[p].showOutline),
            Outline::Frozen(f) => (g.freezes[f].physical, FROZEN_Z, g.freezes[f].outline),
        };
        let p = &g.physicals[p];
        let [x, y, _] = p.transform.position;
        let [qx, qy, qz, qw] = p.transform.rotation;
        let s = p.transform.localScale;
        *transform = Transform::from_xyz(x, y, z).with_rotation(Quat::from_xyzw(qx, qy, qz, qw)).with_scale(Vec3::new(s[0], s[1], 1.0));
        let shown = on && p.active && !p.isDisintegrated && !p.destroyed;
        *visibility = if shown { Visibility::Inherited } else { Visibility::Hidden };
    }
}
