//! Extras (not the game's): phase loss's drawing, on top of mishap_view.rs's see-through tint. Each phased limb
//! shimmers (its sprite wavers sideways) and glows in the shot's colour, the glow swelling just before it snaps back.

use crate::lights::Glow;
use crate::mishap_view::{game_light, noise, COLOURS};
use crate::play::{LimbSprite, Play, Textures};
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

/// The glow of the k-th phased limb.
#[derive(Component)]
#[require(crate::play::MainScene)]
pub struct PhaseGlow(usize);

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn draw(
    mut commands: Commands,
    play: Res<Play>,
    time: Res<Time>,
    mut images: ResMut<Assets<Image>>,
    mut textures: ResMut<Textures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut glows: ResMut<Assets<Glow>>,
    mut art: Local<Option<(Handle<Mesh>, Handle<Image>)>>,
    mut sprites: Query<(&LimbSprite, &mut Transform), Without<PhaseGlow>>,
    mut lights: Query<(Entity, &PhaseGlow, &mut Transform, &MeshMaterial2d<Glow>), Without<LimbSprite>>,
) {
    let ghosts = play.game.phase_ghosts();
    if ghosts.is_empty() && lights.is_empty() {
        return;
    }
    let (quad, soft) = art.get_or_insert_with(|| (meshes.add(Rectangle::new(1.0, 1.0)), game_light(&play, &mut images, &mut textures))).clone();
    let now = time.elapsed_secs();
    // The shimmer: a phased limb's sprite wavers sideways.
    for (&LimbSprite(l), mut tf) in &mut sprites {
        if ghosts.iter().any(|g| g.0 == l) {
            tf.translation.x += 0.035 * (now * 31.0 + l as f32 * 2.3).sin();
        }
    }
    let have = lights.iter().count();
    for k in have..ghosts.len() {
        let m = glows.add(Glow { color: Vec4::ZERO, texture: soft.clone() });
        commands.spawn((PhaseGlow(k), Mesh2d(quad.clone()), MeshMaterial2d(m), Transform::default(), RenderLayers::layer(1)));
    }
    for (e, &PhaseGlow(k), mut tf, material) in &mut lights {
        let Some(&(l, at, colour, left)) = ghosts.get(k) else {
            commands.entity(e).despawn();
            continue;
        };
        let soon = (1.0 - left / 0.4).clamp(0.0, 1.0);
        let flick = 0.75 + 0.5 * noise((now * 20.0) as u32, l as u32);
        *tf = Transform::from_translation(Vec2::from(at).extend(0.0)).with_scale(Vec3::splat(1.5 + 1.0 * soon));
        if let Some(mut m) = glows.get_mut(&material.0) {
            m.color = COLOURS[colour].extend(1.0) * (1.3 + 2.0 * soon) * flick;
        }
    }
}
