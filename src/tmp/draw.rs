//! Draws `Game::world_texts` (game/world_text.rs): TextMeshPro.OnPreRenderObject's mesh, through the text's
//! anchor, with its material (tmp::material, rewritten from the install's TextMeshPro shaders).
use bevy::{camera::visibility::NoFrustumCulling, mesh::{Indices, PrimitiveTopology}, prelude::*, asset::RenderAssetUsages};

use super::material::{text_material, TextMaterial, ATTRIBUTE_UV2};
use super::dynamic::Dynamic;
use super::TextMesh;
use std::sync::Arc;
use crate::play::{load_texture, render_queue, sprite_line_key, Play, Textures};

/// The text mesh in world space: quads through the anchor's transform (x and y axes, as the camera is orthographic
/// down z), two triangles each as TMP's index buffer (0 1 2, 2 3 0).
fn mesh_of(t: &TextMesh, local: [[f32; 2]; 3], z: f32) -> Mesh {
    quads_mesh(&t.quads[..t.main_len], local, z)
}

fn quads_mesh(quads: &[super::Quad], local: [[f32; 2]; 3], z: f32) -> Mesh {
    let [o, ax, ay] = local;
    let n = quads.len() * 4;
    let (mut pos, mut uv, mut uv2, mut col, mut idx) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(quads.len() * 6));
    for (k, q) in quads.iter().enumerate() {
        for c in 0..4 {
            let p = q.pos[c];
            pos.push([o[0] + ax[0] * p[0] + ay[0] * p[1], o[1] + ax[1] * p[0] + ay[1] * p[1], z]);
            uv.push(q.uv[c]);
            uv2.push(q.uv2[c]);
            col.push(q.color[c].map(|x| x as f32 / 255.0));
        }
        let b = (k * 4) as u32;
        idx.extend([b, b + 1, b + 2, b + 2, b + 3, b]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(ATTRIBUTE_UV2, uv2)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(Indices::U32(idx))
}

/// A dynamic font asset's atlas as an Image (Alpha8 sampled as the alpha of Rgba8, rows top first), the Texture2D's bilinear filter and
/// clamp-less wrap (filter 1, wrap 0); a texture still 0x0 is one transparent pixel.
fn dynamic_image(d: &Dynamic) -> Image {
    use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let (w, h, data) = match d.atlas() {
        Some(a) => {
            let rows: Vec<&[u8]> = a.pixels.chunks(a.width as usize).rev().collect();
            (a.width as u32, a.height as u32, rows.concat().iter().flat_map(|&v| [0, 0, 0, v]).collect::<Vec<u8>>())
        }
        None => (1, 1, vec![0; 4]),
    };
    let mut image = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8Unorm, RenderAssetUsages::default());
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor { address_mode_u: ImageAddressMode::Repeat, address_mode_v: ImageAddressMode::Repeat, mag_filter: ImageFilterMode::Linear, min_filter: ImageFilterMode::Linear, ..default() });
    image
}

/// The shared Image of a dynamic atlas, made once per font asset.
fn dynamic_atlas(d: &Arc<Dynamic>, dynamics: &mut Vec<(Arc<Dynamic>, Handle<Image>, u64)>, images: &mut Assets<Image>) -> Handle<Image> {
    if let Some((_, h, _)) = dynamics.iter().find(|(x, _, _)| Arc::ptr_eq(x, d)) {
        return h.clone();
    }
    let version = d.atlas_version();
    let h = images.add(dynamic_image(d));
    dynamics.push((d.clone(), h.clone(), version));
    h
}

pub struct Drawn {
    entity: Entity,
    mesh: Handle<Mesh>,
    last: Option<[[f32; 2]; 3]>,
    /// The TMP_SubMesh children: entity, mesh, and the material they were made for.
    subs: Vec<(Entity, Handle<Mesh>, Option<crate::assets::db::ObjRef>, Option<crate::assets::db::ObjRef>)>,
}

#[allow(clippy::too_many_arguments)]
pub fn world_texts(
    mut commands: Commands,
    mut play: ResMut<Play>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<TextMaterial>>,
    mut textures: ResMut<Textures>,
    mut drawn: Local<Vec<Drawn>>,
    mut visibility: Query<&mut Visibility>,
) {
    if play.is_added() {
        drawn.clear();
    }
    // OnPreRenderObject: regenerate the changed texts first.
    let mut changed = vec![false; play.game.world_texts.len()];
    for (i, c) in changed.iter_mut().enumerate() {
        *c = play.game.world_texts[i].dirty;
        play.game.text_update_mesh(i);
        // TMP_SpriteAnimator UpdateGeometry since the last draw.
        *c |= std::mem::take(&mut play.game.world_texts[i].geometry_changed);
    }
    let g = &play.game;
    let Some(db) = g.assets.as_deref() else { return };
    while drawn.len() < g.world_texts.len() {
        let i = drawn.len();
        let t = &g.world_texts[i];
        let Some(p) = &t.props else { return };
        let atlas = p.font.atlas.and_then(|a| load_texture(db, &mut images, &mut textures, a)).map(|x| x.0).ok_or("TMP font asset without its atlas").unwrap();
        let material = materials.add(text_material(&p.material, atlas));
        let mesh = meshes.add(mesh_of(&TextMesh::default(), [[0.0; 2], [1.0, 0.0], [0.0, 1.0]], 0.0));
        let entity = commands
            .spawn((
                crate::play::MainScene,
                Mesh2d(mesh.clone()),
                MeshMaterial2d(material),
                Transform::default(),
                sprite_line_key(t.layer, t.order, render_queue(db, p.material.material), i),
                crate::lights::camera_layers(t.game_layer),
                NoFrustumCulling,
                Visibility::Hidden,
            ))
            .id();
        drawn.push(Drawn { entity, mesh, last: None, subs: Vec::new() });
    }
    for (i, t) in g.world_texts.iter().enumerate() {
        let alive = match t.anchor {
            crate::game::BareAnchor::Physical(p) => g.physicals[p].active && !g.physicals[p].isDisintegrated && !g.physicals[p].destroyed,
            _ => true,
        };
        let visible = alive && t.visible && t.enabled && t.renderer_enabled && t.props.is_some();
        let d = &mut drawn[i];
        if let Ok(mut v) = visibility.get_mut(d.entity) {
            *v = if visible { Visibility::Inherited } else { Visibility::Hidden };
        }
        for (e, ..) in &d.subs {
            if let Ok(mut v) = visibility.get_mut(*e) {
                *v = if visible { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
        if !visible {
            continue;
        }
        let ([x, y, _], q, s) = g.anchored_world(t.anchor, &t.chain);
        let rot = |v: [f32; 3]| crate::game::mathf::rotate(q, v);
        let (ax, ay) = (rot([s[0], 0.0, 0.0]), rot([0.0, s[1], 0.0]));
        let local = [[x, y], [ax[0], ax[1]], [ay[0], ay[1]]];
        let mesh = &t.mesh;
        if changed[i] || d.last != Some(local) {
            let _ = meshes.insert(d.mesh.id(), mesh_of(mesh, local, 0.0));
            // TMP_Text.GenerateTextMesh: a TMP_SubMesh child per extra material reference, with the parent's sorting and
            // layer, its own material (the fallback clone) and its font's atlas.
            let mut offset = mesh.main_len;
            for (k, sub) in mesh.submeshes.iter().enumerate() {
                if d.subs.get(k).is_some_and(|s| (s.2, s.3) != (sub.material.material, sub.atlas)) {
                    // The sub mesh now draws another material (a sprite asset, a fallback font): replace its child.
                    commands.entity(d.subs[k].0).despawn();
                    d.subs.truncate(k);
                }
                if d.subs.len() <= k {
                    let atlas = sub.atlas.and_then(|a| load_texture(db, &mut images, &mut textures, a)).map(|x| x.0).ok_or("TMP fallback font asset without its atlas").unwrap();
                    let material = materials.add(text_material(&sub.material, atlas));
                    let handle = meshes.add(quads_mesh(&sub.quads, local, 0.0));
                    let entity = commands
                        .spawn((
                            crate::play::MainScene,
                            Mesh2d(handle.clone()),
                            MeshMaterial2d(material),
                            Transform::default(),
                            sprite_line_key(t.layer, t.order, render_queue(db, sub.material.material), i),
                            crate::lights::camera_layers(t.game_layer),
                            NoFrustumCulling,
                            Visibility::Inherited,
                        ))
                        .id();
                    d.subs.push((entity, handle, sub.material.material, sub.atlas));
                } else {
                    let _ = meshes.insert(d.subs[k].1.id(), quads_mesh(&sub.quads, local, 0.0));
                }
                offset += sub.quads.len();
            }
            // Sub meshes the text no longer fills stay as empty children.
            for k in mesh.submeshes.len()..d.subs.len() {
                let _ = meshes.insert(d.subs[k].1.id(), quads_mesh(&[], local, 0.0));
            }
            debug_assert_eq!(offset, mesh.quads.len());
            d.last = Some(local);
        }
    }
}
