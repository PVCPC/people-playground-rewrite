//! ToolInvisibilityTracker.OnMouseEnter's look: the wire's LineRenderer or the pin's SpriteRenderer drawn with
//! Global.main.DeleteWireMaterial (Resources materials/DeleteWire, Unlit/TwoSidedUnlit) on the ScreenUI layer until
//! OnMouseExit puts the initial material and layer back (`InvisibilityTracker::delete_look`).
use crate::play::{load_texture, Play, RendererSprite, Textures};
use crate::wires::{BloodWire, WireLine, WireLineMaterial};
use bevy::{
    asset::embedded_asset,
    camera::visibility::RenderLayers,
    mesh::MeshVertexBufferLayoutRef,
    prelude::*,
    reflect::TypePath,
    render::render_resource::{AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError},
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPipeline, Material2dPlugin},
};

/// Unlit/TwoSidedUnlit: _MainTex at uv * _MainTex_ST.xy + _MainTex_ST.zw, as it is (no tint, Blend Off, Cull Off).
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct DeleteWireMaterial {
    #[uniform(0)]
    st: Vec4,
    #[texture(1)]
    #[sampler(2)]
    texture: Handle<Image>,
}

impl Material2d for DeleteWireMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://people_playground_bevy/delete_wire.wesl".into()
    }

    // The material's render queue is 3000: drawn in order with the transparent ones, but the pass has no Blend.
    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(_: &Material2dPipeline, descriptor: &mut RenderPipelineDescriptor, _: &MeshVertexBufferLayoutRef, _: Material2dKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(Some(target)) = descriptor.fragment.as_mut().and_then(|f| f.targets.first_mut()) {
            target.blend = None;
        }
        Ok(())
    }
}

pub fn add(app: &mut App) {
    embedded_asset!(app, "delete_wire.wesl");
    app.add_plugins(Material2dPlugin::<DeleteWireMaterial>::default());
}

/// The initial material and layer of a renderer drawn with DeleteWire.
#[derive(Component)]
pub struct Initial {
    color: Option<MeshMaterial2d<ColorMaterial>>,
    line: Option<MeshMaterial2d<WireLineMaterial>>,
    blood: Option<MeshMaterial2d<BloodWire>>,
    layers: Option<RenderLayers>,
}

/// A LineRenderer's DeleteWire material (it keeps the material's _MainTex).
pub fn line_material(db: &crate::assets::db::AssetDb, images: &mut Assets<Image>, textures: &mut Textures) -> DeleteWireMaterial {
    let (texture, st) = delete_wire(db, images, textures);
    DeleteWireMaterial { st, texture }
}

/// DeleteWire.mat's _MainTex and _MainTex_ST.
fn delete_wire(db: &crate::assets::db::AssetDb, images: &mut Assets<Image>, textures: &mut Textures) -> (Handle<Image>, Vec4) {
    let r = db.resource("materials/DeleteWire", 21).ok().flatten().expect("Resources materials/DeleteWire");
    let m = crate::assets::components::Material::read(db.data(r)).expect("DeleteWire.mat");
    let (_, t, st) = m.textures.iter().find(|(n, ..)| n == "_MainTex").expect("DeleteWire's _MainTex");
    let texture = load_texture(db, images, textures, db.resolve(r.file, *t).expect("DeleteWire's texture")).expect("DeleteWire's texture").0;
    (texture, Vec4::from_array(*st))
}

/// renderer.sharedMaterial = DeleteWireMaterial and gameObject.layer = ScreenUI while `delete_look`, then back.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn delete_looks(
    mut commands: Commands,
    play: Res<Play>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<DeleteWireMaterial>>,
    colors: Res<Assets<ColorMaterial>>,
    mut textures: ResMut<Textures>,
    mut look: Local<Option<(Handle<Image>, Vec4)>>,
    entities: Query<(Entity, Option<&WireLine>, Option<&RendererSprite>, Option<&MeshMaterial2d<ColorMaterial>>, Option<&MeshMaterial2d<WireLineMaterial>>, Option<&MeshMaterial2d<BloodWire>>, Option<&RenderLayers>, Option<&Initial>), Or<(With<WireLine>, With<RendererSprite>)>>,
) {
    let g = &play.game;
    let Some(db) = g.assets.as_deref() else { return };
    for (entity, line, renderer, color, wire_line, blood, layers, initial) in &entities {
        let on = match (line, renderer) {
            (Some(&WireLine::Wire(w)), _) => g.blood_wires.get(w).is_some_and(|x| x.tracker.delete_look),
            (_, Some(r)) => g.pins.iter().any(|p| p.renderer == Some(r.0) && p.tracker.delete_look),
            _ => false,
        };
        let mut e = commands.entity(entity);
        match (on, initial) {
            (true, None) => {
                let (main, st) = look.get_or_insert_with(|| delete_wire(db, &mut images, &mut textures)).clone();
                // A SpriteRenderer gives _MainTex its sprite's texture; a LineRenderer keeps the material's.
                let texture = match (renderer, color) {
                    (Some(_), Some(c)) => colors.get(&c.0).and_then(|c| c.texture.clone()).unwrap_or_default(),
                    (Some(_), None) => unimplemented!("mod-only: ToolInvisibilityTracker.OnMouseEnter: a pin sprite drawn with a custom material"),
                    _ => main,
                };
                e.insert(Initial { color: color.cloned(), line: wire_line.cloned(), blood: blood.cloned(), layers: layers.cloned() });
                e.remove::<(MeshMaterial2d<ColorMaterial>, MeshMaterial2d<WireLineMaterial>, MeshMaterial2d<BloodWire>)>();
                e.insert((MeshMaterial2d(materials.add(DeleteWireMaterial { st, texture })), crate::lights::camera_layers(16)));
            }
            (false, Some(i)) => {
                e.remove::<(MeshMaterial2d<DeleteWireMaterial>, Initial, RenderLayers)>();
                if let Some(m) = &i.color {
                    e.insert(m.clone());
                }
                if let Some(m) = &i.line {
                    e.insert(m.clone());
                }
                if let Some(m) = &i.blood {
                    e.insert(m.clone());
                }
                if let Some(l) = &i.layers {
                    e.insert(l.clone());
                }
            }
            _ => {}
        }
    }
}
