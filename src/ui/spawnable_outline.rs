//! SpawnableOutlineBehaviour: the shown contraption outline, a RectPrefab SpriteRenderer (Sliced) per RotatedRectangle under the object that
//! CatalogBehaviour.Update keeps at Global.main.MousePosition, mirrored by its localScale.x.

use super::{art::UiArt, state::Ui};
use crate::{game::contraption::RotatedRectangle, play::Play};
use bevy::{prelude::*, sprite::{BorderRect, SliceScaleMode, SpriteImageMode, TextureSlicer}};

/// The SpawnableOutline object.
#[derive(Component)]
pub struct SpawnableOutline;

/// Above every ranked sprite (they sit in -0.5 to 0.5).
const Z: f32 = 0.7;

/// SpawnableOutlineBehaviour.SetOutline and CatalogBehaviour.Update's SpawnableOutline.transform.position.
pub fn spawnable_outline(
    mut commands: Commands,
    ui: Res<Ui>,
    art: Option<Res<UiArt>>,
    play: Option<Res<Play>>,
    existing: Query<Entity, With<SpawnableOutline>>,
    mut shown: Local<Option<(Vec<RotatedRectangle>, f32)>>,
) {
    let (Some(art), Some(play)) = (art, play) else { return };
    let Some((rects, scale)) = ui.local.outline.clone() else {
        if shown.take().is_some() {
            for e in &existing {
                commands.entity(e).despawn();
            }
        }
        return;
    };
    let Some((rect, border, ppu, color)) = art.outline_rect.clone() else { return };
    let m = play.game.MousePosition;
    let at = Transform { translation: Vec3::new(m[0], m[1], Z), scale: Vec3::new(scale, 1.0, 1.0), ..default() };
    if shown.as_ref() == Some(&(rects.clone(), scale)) {
        for e in &existing {
            commands.entity(e).insert(at);
        }
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    // Unity's Sliced border is in sprite pixels over pixels-per-unit; Bevy's slicer works in the image's pixels, so each child is scaled by 1 / ppu.
    let slicer = TextureSlicer { border: BorderRect { min_inset: Vec2::new(border[0], border[3]), max_inset: Vec2::new(border[2], border[1]) }, center_scale_mode: SliceScaleMode::Stretch, sides_scale_mode: SliceScaleMode::Stretch, max_corner_scale: 1.0 };
    commands.spawn((SpawnableOutline, at, Visibility::default())).with_children(|p| {
        for r in &rects {
            p.spawn((
                Sprite { image: rect.image.clone(), rect: Some(rect.rect), color, custom_size: Some(Vec2::new(r.Size[0], r.Size[1]) * ppu), image_mode: SpriteImageMode::Sliced(slicer.clone()), ..default() },
                Transform { translation: Vec3::new(r.Center[0], r.Center[1], 0.0), rotation: Quat::from_rotation_z(r.AngleDegrees.to_radians()), scale: Vec3::splat(1.0 / ppu) },
            ));
        }
    });
    *shown = Some((rects, scale));
}
