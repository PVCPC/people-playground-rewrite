//! Menus-and-UI scripts that only change what is drawn: PanelsBehaviour (the map's background panels).
#![allow(non_snake_case)]

use super::visual::SpriteAt;
use super::Game;
use super::spawn::Instance;
use crate::assets::db::{Component, Prefab};

/// The state of those scripts.
#[derive(Clone, Debug, Default)]
pub struct UiVisual {
    /// Each live PanelsBehaviour's own SpriteRenderer, in link order (FindObjectOfType finds the first).
    pub panels: Vec<SpriteAt>,
}

impl Game {
    /// A PanelsBehaviour of a linked prefab: GetComponent<SpriteRenderer>() of its object, kept for Start and Refresh.
    pub(super) fn panels_link(&mut self, prefab: &Prefab, inst: &Instance, o: usize) {
        let k = prefab.objects[o].components.iter().position(|(_, y)| matches!(y, Component::SpriteRenderer(_))).unwrap_or_else(|| super::menu::throw("NullReferenceException: PanelsBehaviour without a SpriteRenderer"));
        let rid = inst.objects[o].components[k];
        let r = self.renderers.iter().position(|r| r.id == rid).unwrap_or_else(|| unimplemented!("mod-only: PanelsBehaviour on a PhysicalBehaviour's object"));
        self.ui_visual.panels.push(SpriteAt::Renderer(r));
    }

    /// PanelsBehaviour.Start and PanelsBehaviour.Refresh: SpriteRenderer.color = SimplifiedBackground ? Color.clear : Color.white.
    /// Start runs it on each; VideoSettingsInitBehaviour.Sync (after every SimplifiedBackground change, a radio button's
    /// ApplySetting) on FindObjectOfType's. Each map prefab has one and the Main scene none, so the UI calling this each
    /// frame with the preference gives the same picture.
    pub fn panels_refresh(&mut self, simplified: bool) {
        let c = if simplified { [0.0; 4] } else { [1.0; 4] };
        for s in self.ui_visual.panels.clone() {
            if self.sprite_color(s) != c {
                self.set_sprite_color(s, c);
            }
        }
    }
}
