//! The UI side of the Menus-and-UI drawing scripts (game::ui_visual): what they read from the preferences each frame.

use super::state::Settings;
use crate::play::Play;
use bevy::prelude::*;

/// PanelsBehaviour.Refresh from SimplifiedBackground (VideoSettingsInitBehaviour.Sync runs it after every change).
pub fn panels(settings: Res<Settings>, mut play: ResMut<Play>) {
    let simplified = settings.flag("SimplifiedBackground");
    play.game.panels_refresh(simplified);
}

/// _Time.y of every Unlit/Jaap/PanelsShader material: the loaded scene's Time.timeSinceLevelLoad (Main: the game's clock;
/// Menu: MenuAudio.level_time, or PPG_LEVEL_TIME for the rendering check).
pub fn panels_time(play: Option<Res<Play>>, menu: Option<Res<super::MenuAudio>>, mut panels: ResMut<Assets<crate::lights::Panels>>) {
    let t = match (&play, &menu) {
        (Some(p), _) => p.game.time.time,
        (None, Some(m)) => std::env::var("PPG_LEVEL_TIME").ok().and_then(|v| v.parse().ok()).unwrap_or(m.level_time as f32),
        _ => return,
    };
    let stale: Vec<_> = panels.iter().filter(|(_, p)| p.settings.time != t).map(|(id, _)| id).collect();
    for id in stale {
        if let Some(mut p) = panels.get_mut(id) {
            p.settings.time = t;
        }
    }
}

/// The Menu scene's "Panels" while it is loaded (no game): FitCameraBackgroundBehaviour.OnWillRenderObject (position = the
/// camera's x and y, localScale = orthographicSize * 2 * (aspect, 1) / the sprite's size) and PanelsBehaviour.Start and Refresh
/// (colour clear when SimplifiedBackground, else white). The scene's Main Camera sits at (0, 0) with orthographicSize 2.
#[derive(Component)]
pub struct MenuPanels;

pub fn menu_panels(
    mut commands: Commands,
    play: Option<Res<Play>>,
    menu: Option<Res<super::MenuAudio>>,
    art: Res<super::art::UiArt>,
    settings: Res<Settings>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    mut camera: Single<(&mut Transform, &mut Projection), (With<crate::lights::MainCamera>, Without<MenuPanels>)>,
    mut sprite: Query<(Entity, &mut Transform, &MeshMaterial2d<crate::lights::Panels>), With<MenuPanels>>,
    mut panels: ResMut<Assets<crate::lights::Panels>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Some((material, size)) = art.menu_panels.as_ref().filter(|_| play.is_none() && menu.is_some()) else {
        sprite.iter().for_each(|(e, ..)| commands.entity(e).despawn());
        return;
    };
    let (transform, projection) = &mut *camera;
    let Projection::Orthographic(o) = &mut **projection else { return };
    (transform.translation.x, transform.translation.y, o.scale) = (0.0, 0.0, 2.0);
    let aspect = window.physical_width().max(1) as f32 / window.physical_height().max(1) as f32;
    let at = Transform { translation: Vec3::new(0.0, 0.0, -1.0), scale: Vec3::new(o.scale * 2.0 * aspect / size[0], o.scale * 2.0 / size[1], 1.0), ..default() };
    let alpha = if settings.flag("SimplifiedBackground") { 0.0 } else { 1.0 };
    match sprite.single_mut() {
        Ok((_, mut t, m)) => {
            *t = at;
            if panels.get(&m.0).is_some_and(|p| p.settings.alpha != alpha) {
                panels.get_mut(&m.0).unwrap().settings.alpha = alpha;
            }
        }
        Err(_) => {
            let mut p = material.clone();
            p.settings.alpha = alpha;
            commands.spawn((MenuPanels, at, Mesh2d(meshes.add(Rectangle::new(size[0], size[1]))), MeshMaterial2d(panels.add(p))));
        }
    }
}
