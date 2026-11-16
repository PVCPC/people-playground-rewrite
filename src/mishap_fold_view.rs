//! Extras (not the game's): a spatial fold's limb is drawn clipped at its exit rim, so it comes out of the host's
//! surface through the tiny portal instead of overlapping it (src/game/mishap_fold.rs is the game side; the rims
//! themselves are drawn by mishap_view.rs). A limb in a big portal's doorway is left to extras_view's clipping.

use crate::extras_view::{clip_mesh, Clipped};
use crate::play::{LimbSprite, Play};
use bevy::prelude::*;

/// A folded limb's sprite: its own mesh and the clipped one it shows.
#[derive(Component)]
pub struct FoldClip {
    orig: Handle<Mesh>,
    shown: Handle<Mesh>,
}

pub fn add(app: &mut App) {
    app.add_systems(Update, clip.after(crate::extras_view::doorway).run_if(resource_exists::<Play>));
}

fn clip(mut commands: Commands, play: Res<Play>, mut meshes: ResMut<Assets<Mesh>>, mut sprites: Query<(Entity, &LimbSprite, &mut Mesh2d, &Transform, Option<&FoldClip>), Without<Clipped>>) {
    let clips = play.game.mishap_fold_clips();
    for (e, &LimbSprite(l), mut mesh, tf, clipped) in &mut sprites {
        // The sprite's own mesh (it may have been swapped since: a new one is its own again).
        let orig = match clipped {
            Some(c) if mesh.0 == c.shown => c.orig.clone(),
            _ => mesh.0.clone(),
        };
        let Some(&(_, rim)) = clips.iter().find(|c| c.0 == l) else {
            if clipped.is_some() {
                mesh.0 = orig;
                commands.entity(e).remove::<FoldClip>();
            }
            continue;
        };
        let Some(front) = meshes.get(&orig).and_then(|m| clip_mesh(m, tf, rim)) else { continue };
        match clipped {
            Some(c) => {
                if let Some(mut m) = meshes.get_mut(&c.shown) {
                    *m = front;
                }
                mesh.0 = c.shown.clone();
                commands.entity(e).insert(FoldClip { orig, shown: c.shown.clone() });
            }
            None => {
                let shown = meshes.add(front);
                mesh.0 = shown.clone();
                commands.entity(e).insert(FoldClip { orig, shown });
            }
        }
    }
}
