//! The LineRenderers of `Game::lines` (game/line.rs): meshed like the wires' (wires::line_mesh) and drawn with the
//! line's own material from the install: Particles/Standard Unlit through particles::particle_material, or a shader of
//! custom_material.rs (its values and textures from the .mat, the line's colours as the vertex colour).
use crate::assets::components::Material;
use crate::game::line::LineVisual;
use crate::particles::{particle_material, ParticleMaterial, UV2_BLEND};
use crate::play::{load_texture, render_queue, sprite_line_key, Play, Textures};
use crate::wires::{line_mesh_full, LineTexture};
use bevy::{camera::visibility::NoFrustumCulling, mesh::VertexAttributeValues, prelude::*};

/// What a line shows: its world-space points, or its local ones with the transform to world.
struct Shown {
    points: Vec<[f32; 2]>,
    local: Option<[[f32; 2]; 3]>,
    visible: bool,
}

fn shown(g: &crate::game::Game, l: &LineVisual) -> Shown {
    let alive = match l.anchor {
        crate::game::BareAnchor::Physical(p) => g.physicals[p].active && !g.physicals[p].isDisintegrated && !g.physicals[p].destroyed,
        _ => true,
    };
    let visible = l.visible && l.enabled && alive && l.positions.len() >= 2;
    let points = l.positions.iter().map(|p| [p[0], p[1]]).collect();
    let local = (!l.use_world_space).then(|| {
        let ([x, y, _], q, s) = g.anchored_world(l.anchor, &l.chain);
        let rot = |v: [f32; 3]| crate::game::mathf::rotate(q, v);
        let (ax, ay) = (rot([s[0], 0.0, 0.0]), rot([0.0, s[1], 0.0]));
        [[x, y], [ax[0], ax[1]], [ay[0], ay[1]]]
    });
    Shown { points, local, visible }
}

/// The z of each position in Local alignment's camera space (LineRendererGeometryJob::Initialize 0x180ea5c00): a camera on the
/// object's own axes (right, up, -forward), backed off along its forward by the renderer bounds' largest half extent plus 1.
/// BuildSmoothCorner and BuildCap flip their side when that z is positive.
fn local_camera_z(g: &crate::game::Game, l: &LineVisual) -> Vec<f32> {
    use crate::game::mathf::rotate;
    let ([tx, ty, tz], q, sc) = g.anchored_world(l.anchor, &l.chain);
    let (x, y, z) = (rotate(q, [sc[0], 0.0, 0.0]), rotate(q, [0.0, sc[1], 0.0]), rotate(q, [0.0, 0.0, sc[2]]));
    let world: Vec<[f32; 3]> = l.positions.iter().map(|p| if l.use_world_space { *p } else { std::array::from_fn(|k| [tx, ty, tz][k] + x[k] * p[0] + y[k] * p[1] + z[k] * p[2]) }).collect();
    camera_z(&world, [tx, ty, tz], x, y, z)
}

fn camera_z(world: &[[f32; 3]], [tx, ty, tz]: [f32; 3], x: [f32; 3], y: [f32; 3], z: [f32; 3]) -> Vec<f32> {
    let (mut lo, mut hi) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for p in world {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let e = (0..3).map(|k| (hi[k] - lo[k]) * 0.5).fold(0.0, f32::max);
    let cross = [x[1] * y[2] - x[2] * y[1], x[2] * y[0] - x[0] * y[2], x[0] * y[1] - x[1] * y[0]];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let det = dot(z, cross);
    world
        .iter()
        .map(|p| {
            let origin: [f32; 3] = std::array::from_fn(|k| [tx, ty, tz][k] - (e + 1.0) * z[k]);
            if det.abs() < 1e-12 { 0.0 } else { -dot(std::array::from_fn(|k| p[k] - origin[k]), cross) / det }
        })
        .collect()
}

fn mesh_of(g: &crate::game::Game, l: &LineVisual, s: &Shown) -> Mesh {
    let mode = match l.texture_mode {
        0 => LineTexture::Stretch,
        1 => LineTexture::Tile,
        3 => LineTexture::RepeatPerSegment,
        2 => LineTexture::DistributePerSegment,
        m => unimplemented!("mod-only: LineRenderer textureMode {m}"),
    };
    // View (0) builds in the camera's space, where a line's z is negative; Local (1) in local_camera_z's.
    let zs = if l.alignment == 1 { local_camera_z(g, l) } else { Vec::new() };
    let mut mesh = line_mesh_full(&s.points, &zs, l.width, &l.curve, Some(&l.gradient), l.cap_vertices, l.corner_vertices.max(0) as usize, l.looped, mode);
    let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION) else { unreachable!() };
    let given: Vec<[f32; 2]> = positions.iter().map(|p| [p[0], p[1]]).collect();
    if let Some([o, x, y]) = s.local {
        for p in positions.iter_mut() {
            *p = [o[0] + x[0] * p[0] + y[0] * p[1], o[1] + x[1] * p[0] + y[1] * p[1], p[2]];
        }
    }
    let n = positions.len();
    mesh.insert_attribute(UV2_BLEND, vec![[0.0f32; 3]; n]);
    mesh.insert_attribute(crate::custom_material::LINE_LOCAL, given);
    mesh
}

/// The entity of `Game::lines[i]`, what its mesh was built from, and the material and GameObject layer it was spawned with.
pub struct Drawn {
    entity: Entity,
    mesh: Handle<Mesh>,
    last: Option<(Vec<[f32; 2]>, Option<[[f32; 2]; 3]>, f32, crate::game::visual::Gradient)>,
    look: (Option<crate::assets::db::ObjRef>, i32),
}

/// The line's entity with its material: Particles/Standard Unlit, a custom_material.rs shader, or Unlit/TwoSidedUnlit (DeleteWire).
#[allow(clippy::too_many_arguments)]
fn spawn(
    commands: &mut Commands,
    g: &crate::game::Game,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ParticleMaterial>,
    delete_wires: &mut Assets<crate::delete_wire::DeleteWireMaterial>,
    textures: &mut Textures,
    i: usize,
) -> Drawn {
    let db = g.assets.as_deref().unwrap();
    let l = &g.lines[i];
    let Some(r) = l.material else { unimplemented!("mod-only: LineRenderer without a material (the default line material)") };
    let m = Material::read(db.data(r)).unwrap();
    let main = m.textures.iter().find(|(n, ..)| n == "_MainTex");
    let source = main.and_then(|(_, t, _)| db.resolve(r.file, *t));
    let texture = source.and_then(|s| load_texture(db, images, textures, s)).map(|t| t.0).unwrap_or_default();
    let s = shown(g, l);
    let mesh = meshes.add(mesh_of(g, l, &s));
    let mut e = commands.spawn((
        crate::play::MainScene,
        Mesh2d(mesh.clone()),
        Transform::default(),
        sprite_line_key(l.layer, l.order, render_queue(db, Some(r)), i),
        crate::lights::camera_layers(l.game_layer),
        NoFrustumCulling,
        if s.visible { Visibility::Inherited } else { Visibility::Hidden },
    ));
    let shader = db.resolve(r.file, m.shader).map(|s| s.path_id);
    match shader.and_then(crate::custom_material::def_for) {
        // custom_sync gives it its material.
        Some(def) => e.insert((crate::custom_material::new_sprite(def, db, images, textures, r.file, &m, texture, Vec4::ZERO).expect("a line shader's textures"), crate::custom_material::LineSprite)),
        // LinkDeviceBehaviour.OnMouseOverlapEvent's Resources "Materials/DeleteWire".
        None if shader == Some(130) => e.insert(MeshMaterial2d(delete_wires.add(crate::delete_wire::line_material(db, images, textures)))),
        None => {
            let st = main.map_or([1.0, 1.0, 0.0, 0.0], |t| t.2);
            e.insert(MeshMaterial2d(materials.add(particle_material(&m, st, texture, false))))
        }
    };
    Drawn { entity: e.id(), mesh, last: None, look: (l.material, l.game_layer) }
}

#[allow(clippy::too_many_arguments)]
pub fn lines(
    mut commands: Commands,
    play: Res<Play>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ParticleMaterial>>,
    mut delete_wires: ResMut<Assets<crate::delete_wire::DeleteWireMaterial>>,
    mut textures: ResMut<Textures>,
    mut drawn: Local<Vec<Drawn>>,
    mut visibility: Query<&mut Visibility>,
    mut keys: Query<&mut crate::play::SortKey>,
    mut sprite_layers: Query<(&crate::play::RendererSprite, &mut bevy::camera::visibility::RenderLayers)>,
) {
    if play.is_added() {
        drawn.clear();
    }
    let g = &play.game;
    if g.assets.is_none() {
        return;
    }
    while drawn.len() < g.lines.len() {
        let i = drawn.len();
        let d = spawn(&mut commands, g, &mut images, &mut meshes, &mut materials, &mut delete_wires, &mut textures, i);
        drawn.push(d);
    }
    // A line given another material or GameObject layer at run time (LinkDeviceBehaviour.OnMouseOverlapEvent): spawned again.
    for (i, l) in g.lines.iter().enumerate() {
        if drawn[i].look != (l.material, l.game_layer) {
            commands.entity(drawn[i].entity).despawn();
            drawn[i] = spawn(&mut commands, g, &mut images, &mut meshes, &mut materials, &mut delete_wires, &mut textures, i);
        }
    }
    // The link end sprites' GameObject layer set at run time (VisualDeletableDetachedToolBehaviour.OnMouseOverlapEvent).
    let ends: Vec<usize> = g.phase_links.iter().flat_map(|l| l.end_renderers.into_iter().flatten()).collect();
    if !ends.is_empty() {
        for (r, mut layers) in &mut sprite_layers {
            if ends.contains(&r.0) {
                let want = crate::lights::camera_layers(g.renderers[r.0].game_layer);
                if *layers != want {
                    *layers = want;
                }
            }
        }
    }
    for (i, l) in g.lines.iter().enumerate() {
        let s = shown(g, l);
        let d = &mut drawn[i];
        if let Ok(mut v) = visibility.get_mut(d.entity) {
            *v = if s.visible { Visibility::Inherited } else { Visibility::Hidden };
        }
        // sortingLayerID and sortingOrder set at run time (SortingLayerChild).
        if let Ok(mut key) = keys.get_mut(d.entity) {
            crate::play::resort(&mut key, l.layer, l.order);
        }
        let built = (s.points.clone(), s.local, l.width, l.gradient.clone());
        if s.visible && d.last.as_ref() != Some(&built) {
            let _ = meshes.insert(d.mesh.id(), mesh_of(g, l, &s));
            d.last = Some(built);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_camera_sits_behind_the_bounds_and_a_far_point_flips() {
        let axes = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        let z = camera_z(&[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 0.0, -50.0]], [0.0; 3], axes.0, axes.1, axes.2);
        // Half extent 25: the camera is at z = -26 looking along +z; the point at -50 is behind it.
        assert!(z[0] < 0.0 && z[1] < 0.0 && z[2] > 0.0, "{z:?}");
    }
}
