//! The Main scene's "Temperature display container" (ExistInThermalVision) and its "Thermal vision temperature display"
//! (a SpriteRenderer, ThermalVisionTempDisplayBehaviour and IgnoreCameraZoom) with the child "Text (TMP)": drawn as a
//! `Game::renderers` entry and a `Game::world_texts` entry anchored at the root, their chains the scene's Transforms.
use super::Game;
use crate::assets::{components::{Material, MeshRenderer, SpriteRenderer}, db::{AssetDb, ObjRef}, mono::Value};
use crate::unity::transform::Trs;

/// The container's ExistInThermalVision in `thermal_views` (a scene component has no instance id of the rewrite's).
pub const CONTAINER: i32 = i32::MIN / 2;

#[derive(Clone, Debug)]
pub struct ThermalDisplay {
    /// The container's activeSelf (the display and the text are active in the scene).
    pub active: bool,
    pub started: bool,
    /// ThermalVisionTempDisplayBehaviour.ToRead
    pub to_read: u32,
    /// IgnoreCameraZoom.SizeMultiplier (CameraToConsider is the main camera).
    pub size_multiplier: f32,
    /// Its `renderers` and `world_texts` entries (ThermalVisionTempDisplayBehaviour.Text).
    pub renderer: usize,
    pub text: usize,
}

pub(super) fn trs_of(db: &AssetDb, go: ObjRef) -> Result<Trs, String> {
    let o = db.game_object(go)?;
    let t = o.components.iter().filter_map(|c| db.resolve(go.file, *c)).find(|&c| matches!(db.class(c), 4 | 224)).ok_or("a scene GameObject without a Transform")?;
    let t = crate::assets::classes::Transform::read(db.data(t))?;
    let (p, q, s) = (t.local_position, t.local_rotation, t.local_scale);
    Ok(Trs { t: [p[0], p[1], p[2], 0.0], q, s: [s[0], s[1], s[2], 0.0] })
}

fn component(db: &AssetDb, go: ObjRef, class: i32) -> Option<ObjRef> {
    db.game_object(go).ok()?.components.iter().filter_map(|c| db.resolve(go.file, *c)).find(|&c| db.class(c) == class)
}

impl Game {
    /// The display as the scene loads it: its SpriteRenderer, the Text's TextMeshPro (as link.rs reads a prefab's), the
    /// IgnoreCameraZoom on its object. The container is a root with the identity Transform (Update's
    /// `transform.position = MousePosition` is its local position).
    pub(super) fn thermal_display_load(&mut self, db: &mut AssetDb, go: ObjRef, v: &Value) -> Result<(), String> {
        let file = go.file;
        let display = db.game_object(go)?;
        let container_go = {
            let t = component(db, go, 4).ok_or("the display's Transform")?;
            let father = crate::assets::classes::Transform::read(db.data(t))?.father;
            let f = db.resolve(file, father).ok_or("the display's container")?;
            db.resolve(file, crate::assets::classes::Transform::read(db.data(f))?.game_object).ok_or("the container's GameObject")?
        };
        let container = trs_of(db, container_go)?;
        let identity = Trs { t: [0.0; 4], q: [0.0, 0.0, 0.0, 1.0], s: [1.0, 1.0, 1.0, 0.0] };
        if container != identity || component(db, container_go, 4).and_then(|t| crate::assets::classes::Transform::read(db.data(t)).ok()).is_some_and(|t| db.resolve(file, t.father).is_some()) {
            unimplemented!("mod-only: ThermalVisionTempDisplayBehaviour: a container that is not a root at the identity");
        }
        let container_active = db.game_object(container_go)?.active;
        let to_read = v.get("ToRead").and_then(|m| m.get("m_Bits")).and_then(Value::i64).ok_or("ThermalVisionTempDisplayBehaviour.ToRead")? as u32;
        let mut size_multiplier = None;
        for c in display.components.iter().filter_map(|c| db.resolve(file, *c)).filter(|&c| db.class(c) == 114).collect::<Vec<_>>() {
            if let Some((script, mb, z)) = db.mono(c)? {
                if script.class_name == "IgnoreCameraZoom" {
                    if !mb.enabled {
                        unimplemented!("mod-only: ThermalVisionTempDisplayBehaviour: a disabled IgnoreCameraZoom");
                    }
                    size_multiplier = Some(z.get("SizeMultiplier").and_then(Value::f32).ok_or("IgnoreCameraZoom.SizeMultiplier")?);
                }
            }
        }
        let size_multiplier = size_multiplier.ok_or("the display's IgnoreCameraZoom")?;
        let display_trs = trs_of(db, go)?;
        // The SpriteRenderer, as link.rs takes a prefab's.
        let sr = component(db, go, 212).ok_or("the display's SpriteRenderer")?;
        let r = SpriteRenderer::read(db.data(sr))?;
        let sprite = db.resolve(file, r.sprite).ok_or("the display's sprite")?;
        let bounds = super::link::renderer_bounds(&db.sprite(sprite)?, &r);
        let renderer = self.renderers.len();
        self.renderers.push(super::Renderer {
            anchor: super::BareAnchor::Root,
            chain: vec![display_trs, container],
            sprite: (sprite, r.color, r.sorting_order),
            layer: r.sorting_layer_id,
            game_layer: display.layer as i32,
            material: r.materials.first().and_then(|&m| db.resolve(file, m)),
            id: sr.path_id as i32,
            draw: ([r.flip_x, r.flip_y], r.draw_mode, r.size, r.adaptive_mode_threshold, r.sprite_tile_mode == 1),
            visible: container_active && display.active,
            enabled: r.enabled,
            bounds,
        });
        self.renderer_sync(self.renderers.len() - 1);
        // ThermalVisionTempDisplayBehaviour.Text: the TextMeshPro, its RectTransform and MeshRenderer.
        let tmp = v.get("Text").and_then(Value::pptr).and_then(|p| db.resolve(file, p)).ok_or("ThermalVisionTempDisplayBehaviour.Text")?;
        let (_, mb, tv) = db.mono(tmp)?.ok_or("the display's TextMeshPro")?;
        let text_go = db.resolve(file, mb.game_object).ok_or("the TextMeshPro's GameObject")?;
        let text_obj = db.game_object(text_go)?;
        let rect_ref = component(db, text_go, 224).ok_or("the TextMeshPro's RectTransform")?;
        let rect = crate::assets::classes::RectTransform::read(db.data(rect_ref))?;
        let local = crate::assets::classes::Transform::read(db.data(rect_ref))?;
        let mesh_renderer = MeshRenderer::read(db.data(component(db, text_go, MeshRenderer::CLASS).ok_or("TextMeshPro without its MeshRenderer")?))?;
        let text = match tv.get("m_text") {
            Some(Value::Str(s)) => s.clone(),
            _ => return Err("TextMeshPro.m_text".into()),
        };
        let font_ref = tv.get("m_fontAsset").and_then(Value::pptr).and_then(|p| db.resolve(file, p)).ok_or("the display's TextMeshPro font asset")?;
        let font = self.tmp_font(db, font_ref)?;
        let shared = tv.get("m_sharedMaterial").and_then(Value::pptr).and_then(|p| db.resolve(file, p));
        let material = if shared.is_none() || shared == font.material.material {
            font.material.clone()
        } else {
            let m = shared.unwrap();
            let (mat, keywords) = Material::read_with_keywords(db.data(m))?;
            crate::tmp::SdfMaterial { material: Some(m), shader: crate::tmp::material::shader_name(db.data(db.resolve(m.file, mat.shader).ok_or("the text material shader")?)), floats: mat.floats.into_iter().collect(), colors: mat.colors.into_iter().collect(), keywords: keywords.split_whitespace().map(str::to_string).collect() }
        };
        let props = Some(crate::tmp::TextProps::read(&tv, font, material, rect.size_delta, rect.pivot)?);
        let (p, q, s) = (local.local_position, local.local_rotation, local.local_scale);
        let text_trs = Trs { t: [p[0], p[1], p[2], 0.0], q, s: [s[0], s[1], s[2], 0.0] };
        let text_index = self.world_texts.len();
        self.world_texts.push(super::world_text::WorldText::new(
            super::BareAnchor::Root,
            vec![text_trs, display_trs, container],
            tmp.path_id as i32,
            (0, CONTAINER),
            (text_obj.layer as i32, mesh_renderer.sorting_layer_id, mesh_renderer.sorting_order),
            (container_active && display.active && text_obj.active, mb.enabled, mesh_renderer.enabled),
            text,
            props,
        ));
        let active = container_active && display.active;
        if !text_obj.active {
            unimplemented!("mod-only: ThermalVisionTempDisplayBehaviour: an inactive Text");
        }
        self.thermal_display = Some(ThermalDisplay { active, started: false, to_read, size_multiplier, renderer, text: text_index });
        // Awake order: the active display joins the Update manager, its Start due in the first frame.
        if active {
            self.behaviours.add(0, super::Behaviour::ThermalDisplay);
            self.pending_starts.push(super::misc::PendingStart::ThermalDisplay);
        }
        Ok(())
    }

    /// ExistInThermalVision.Start and ThermalVisionToggled on the container: SetActive(ThermalVisionEnabled). Activating
    /// the display adds ThermalVisionTempDisplayBehaviour to the Update manager (after what is there) and its Start
    /// to the due Starts; deactivating takes it out.
    pub(super) fn thermal_display_set_active(&mut self, on: bool) {
        let Some(d) = self.thermal_display.as_mut() else { return };
        if d.active == on {
            return;
        }
        d.active = on;
        let (r, t, started) = (d.renderer, d.text, d.started);
        self.renderers[r].visible = on;
        self.renderer_sync(r);
        self.world_texts[t].visible = on;
        if on {
            self.behaviours.add(0, super::Behaviour::ThermalDisplay);
            if !started {
                self.pending_starts.push(super::misc::PendingStart::ThermalDisplay);
            }
        } else {
            self.behaviours.remove(super::Behaviour::ThermalDisplay);
        }
    }

    /// ThermalVisionTempDisplayBehaviour.Start: SetText(PhysicalBehaviour.AmbientTemperature).
    pub(crate) fn thermal_display_start(&mut self) {
        let Some(d) = self.thermal_display.as_mut() else { return };
        if d.started || !d.active {
            return;
        }
        d.started = true;
        self.thermal_display_set_text(self.AmbientTemperature);
    }

    /// ThermalVisionTempDisplayBehaviour.Update: unless paused, the object to the mouse, then the temperature under it:
    /// the PhysicalBehaviour of the lowest collider in ToRead at the point (its Temperature when SimulateTemperature,
    /// else the ambient), else a lava's LavaTemperature, else the ambient grid's (AmbientTemperatureTransfer) or the
    /// ambient temperature.
    pub(crate) fn thermal_display_update(&mut self) {
        let Some(d) = self.thermal_display.clone() else { return };
        if self.paused_menu {
            return;
        }
        let m = self.MousePosition;
        self.renderers[d.renderer].chain[0].t = [m[0], m[1], m[2], 0.0];
        self.world_texts[d.text].chain[1].t = [m[0], m[1], m[2], 0.0];
        let point = [m[0], m[1]];
        let hit = self.overlap_point_first(point, d.to_read).and_then(|(p, _)| p).filter(|&p| self.physicals[p].in_world && !self.physicals[p].destroyed);
        let celsius = if let Some(p) = hit {
            if self.physicals[p].SimulateTemperature { self.physicals[p].Temperature } else { self.AmbientTemperature }
        } else if let Some(lv) = self.lava_at_point(point) {
            self.lavas[lv].LavaTemperature
        } else if self.prefs.AmbientTemperatureTransfer {
            self.GetTemperatureAtPoint(point)
        } else {
            self.AmbientTemperature
        };
        self.thermal_display_set_text(celsius);
    }

    /// ThermalVisionTempDisplayBehaviour.SetText: `$"{Math.Round(num, 1)} {suffix} {arg}".Trim()` with num the clamped
    /// temperature in the preferred unit and arg "+" from 99999 °C.
    pub(crate) fn thermal_display_set_text(&mut self, celsius: f32) {
        let Some(d) = &self.thermal_display else { return };
        let t = d.text;
        let num = self.prefs.celsius_to_preference(celsius.clamp(-273.15, 99999.0));
        let arg = if celsius >= 99999.0 { "+" } else { "" };
        let suffix = super::utils::get_temperature_unit_suffix(self.prefs.TemperatureUnit);
        // Math.Round(double, 1): times 10, half to even, over 10.
        let rounded = (num as f64 * 10.0).round_ties_even() / 10.0;
        let text = format!("{} {suffix} {arg}", super::display::cs_double(rounded));
        // string.Trim: white space at both ends.
        self.text_set(t, text.trim());
    }

    /// IgnoreCameraZoom.OnWillRenderObject, as the main camera renders the active display: localScale =
    /// orthographicSize * SizeMultiplier * Vector3.one.
    pub(super) fn thermal_display_will_render(&mut self) {
        let Some(d) = self.thermal_display.clone() else { return };
        if !d.active || !self.renderers[d.renderer].enabled {
            return;
        }
        let k = self.camera.orthographic_size * d.size_multiplier;
        self.renderers[d.renderer].chain[0].s = [k, k, k, 0.0];
        self.world_texts[d.text].chain[1].s = [k, k, k, 0.0];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_text_rounds_clamps_and_marks_overflow() {
        let mut g = Game::default();
        g.world_texts.push(super::super::world_text::WorldText::new(super::super::BareAnchor::Root, vec![], 0, (0, CONTAINER), (0, 0, 0), (true, true, true), String::new(), None));
        g.thermal_display = Some(ThermalDisplay { active: true, started: false, to_read: 0, size_multiplier: 0.04, renderer: 0, text: 0 });
        g.thermal_display_set_text(22.04);
        assert_eq!(g.world_texts[0].text, "22 \u{b0}C");
        g.thermal_display_set_text(36.65);
        assert_eq!(g.world_texts[0].text, "36.7 \u{b0}C");
        g.thermal_display_set_text(150000.0);
        assert_eq!(g.world_texts[0].text, "99999 \u{b0}C +");
        g.thermal_display_set_text(-500.0);
        assert_eq!(g.world_texts[0].text, "-273.1 \u{b0}C");
    }
}
