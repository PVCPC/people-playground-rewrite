//! EnergySwordMotionBlur's trail (game/energy_sword.rs): the mesh Awake makes for the MeshFilter (`mesh = new Mesh()`, kept
//! until OnDestroy's Destroy(mesh)), its vertices, colours and triangles as UpdateMesh last set them, drawn with the
//! MeshRenderer's material while it is enabled and its GameObject active. Its transform is the identity (Update), so the
//! positions are world and in_POSITION0 alike. Only Unlit/EnergySword (custom_material.rs) is drawn.
use crate::assets::components::Material;
use crate::play::{blur_key, render_queue, Play, Textures};
use bevy::{asset::RenderAssetUsages, camera::visibility::NoFrustumCulling, mesh::PrimitiveTopology, prelude::*};

// EnergySwordMotionBlur.Awake: the new Mesh is `Drawn`'s (spawned below); SendToBottomLayer's "Bottom" is game/link.rs's
// EnergySwordMotionBlur.OnDestroy: Destroy(mesh) (the entity stays hidden with the sword's object)
fn mesh_of(b: &crate::game::energy_sword::EnergySwordBlur) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    let given: Vec<[f32; 2]> = b.vertices.iter().map(|v| [v[0], v[1]]).collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, b.vertices.clone());
    // The mesh has no uv: (0, 0).
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; b.vertices.len()]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, b.colors.clone());
    mesh.insert_attribute(crate::custom_material::LINE_LOCAL, given);
    mesh.insert_indices(bevy::mesh::Indices::U32(b.indices.clone()));
    mesh
}

/// The entity of `Game::energy_blurs[i]` (None: not drawn) and the mesh version it shows.
pub struct Drawn {
    entity: Option<(Entity, Handle<Mesh>)>,
    version: u32,
}

#[allow(clippy::too_many_arguments)]
pub fn energy_blurs(
    mut commands: Commands,
    play: Res<Play>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut textures: ResMut<Textures>,
    mut drawn: Local<Vec<Drawn>>,
    mut visibility: Query<&mut Visibility>,
) {
    if play.is_added() {
        drawn.clear();
    }
    let g = &play.game;
    let Some(db) = g.assets.as_deref() else { return };
    while drawn.len() < g.energy_blurs.len() {
        let i = drawn.len();
        let b = &g.energy_blurs[i];
        let Some(r) = b.material else { unimplemented!("mod-only: EnergySwordMotionBlur's MeshRenderer without a material") };
        let m = Material::read(db.data(r)).unwrap();
        let Some(def) = crate::custom_material::def_for(db.material_shader_id(r.file, m.shader)).filter(|d| d.path_id == 905) else {
            // Excalibur's ExcaliburBlurMaterial is the builtin Sprites-Default: not drawn.
            drawn.push(Drawn { entity: None, version: 0 });
            continue;
        };
        let main = m.textures.iter().find(|(n, ..)| n == "_MainTex");
        let texture = main.and_then(|(_, t, _)| db.resolve(r.file, *t)).and_then(|s| crate::play::load_texture(db, &mut images, &mut textures, s)).map(|t| t.0).unwrap_or_default();
        let mesh = meshes.add(mesh_of(b));
        let key = blur_key(b.sorting_layer, b.sorting_order, render_queue(db, Some(r)), i);
        let e = commands.spawn((
            crate::play::MainScene,
            Mesh2d(mesh.clone()),
            Transform::default(),
            key,
            crate::lights::camera_layers(b.game_layer),
            NoFrustumCulling,
            Visibility::Hidden,
            crate::custom_material::new_sprite(def, db, &mut images, &mut textures, r.file, &m, texture, Vec4::ZERO).expect("EnergySword's textures"),
            crate::custom_material::LineSprite,
        ));
        drawn.push(Drawn { entity: Some((e.id(), mesh)), version: b.version });
    }
    for (i, b) in g.energy_blurs.iter().enumerate() {
        let d = &mut drawn[i];
        let Some((entity, mesh)) = &d.entity else { continue };
        // Its object goes with the sword's (as a line's with its physical).
        let alive = match b.A.0 {
            crate::game::BareAnchor::Physical(p) => g.physicals[p].active && !g.physicals[p].isDisintegrated && !g.physicals[p].destroyed,
            _ => true,
        };
        let shown = alive && b.renderer_enabled && b.visible && !b.indices.is_empty();
        if let Ok(mut v) = visibility.get_mut(*entity) {
            *v = if shown { Visibility::Inherited } else { Visibility::Hidden };
        }
        if d.version != b.version {
            let _ = meshes.insert(mesh.id(), mesh_of(b));
            d.version = b.version;
        }
    }
}
