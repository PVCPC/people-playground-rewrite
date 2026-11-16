//! CenterOfGravity and CenterOfBounds: level1's "Center of Gravity" and "Center of Selection" markers, a sprite each
//! that follows the selection's centre of mass (or of its sprite bounds) at a size from the camera. Each has a
//! "Text (TMP)" child whose AppearWhenMouseNear shows the label while the pointer is near the marker.
#![allow(non_snake_case)]

use super::{d, Game};
use crate::assets::db::{AssetDb, Component, ObjRef};
use crate::assets::mono::Value;
use crate::unity::transform::Trs;

/// One marker: its script's fields, its Renderer (a `Game::renderers` entry whose one TRS is the marker's
/// Transform) and its label.
#[derive(Clone, Debug)]
pub struct Center {
    /// CenterOfBounds (its GetPosition override).
    pub bounds: bool,
    pub SizeMultiplier: f32,
    pub renderer: usize,
    pub label: Option<AppearWhenMouseNear>,
}

impl Default for Center {
    fn default() -> Self {
        Self {
            bounds: Default::default(),
            SizeMultiplier: 0.25,
            renderer: Default::default(),
            label: Default::default(),
        }
    }
}

/// AppearWhenMouseNear on a marker's child: Renderer is the child's TextMeshPro MeshRenderer (`world_texts`), Center
/// the marker's Transform, and parentRenderers (ConsiderParentRenderers) the marker's SpriteRenderer.
#[derive(Clone, Debug)]
pub struct AppearWhenMouseNear {
    pub MinDistance: f32,
    pub ConsiderParentRenderers: bool,
    pub text: usize,
}

fn trs(db: &AssetDb, transform: ObjRef) -> Result<Trs, String> {
    let t = crate::assets::classes::Transform::read(db.data(transform))?;
    let (p, s) = (t.local_position, t.local_scale);
    Ok(Trs { t: [p[0], p[1], p[2], 0.0], q: t.local_rotation, s: [s[0], s[1], s[2], 0.0] })
}

impl Game {
    /// CenterOfGravity.Awake: camera = Camera.main (the camera is Game's).
    /// level1's markers (bounds, fields, GameObject): each running script's root object with its SpriteRenderer, and
    /// the AppearWhenMouseNear label below it.
    pub(super) fn load_centers(&mut self, db: &mut AssetDb, scripts: &[(bool, Value, ObjRef)]) -> Result<(), String> {
        for (bounds, fields, go) in scripts {
            let file = go.file;
            let prefab = db.prefab(*go)?;
            if super::spawn::scene_transform(db, *go).map(|t| !t.father.is_null())? {
                unimplemented!("mod-only: CenterOfGravity below another Transform");
            }
            let r = fields.get("Renderer").and_then(Value::pptr).and_then(|p| db.resolve(file, p)).ok_or("CenterOfGravity.Renderer")?;
            let Some(&(0, _)) = prefab.index.get(&r) else { unimplemented!("mod-only: CenterOfGravity.Renderer on another GameObject") };
            let Some((_, Component::SpriteRenderer(sr))) = prefab.objects[0].components.iter().find(|(c, _)| *c == r) else { unimplemented!("mod-only: CenterOfGravity.Renderer that is no SpriteRenderer") };
            let sprite = db.resolve(file, sr.sprite).ok_or("CenterOfGravity.Renderer without a sprite")?;
            let root = trs(db, prefab.objects[0].components[0].0)?;
            let renderer = self.renderers.len();
            self.renderers.push(super::Renderer {
                anchor: super::BareAnchor::Root,
                chain: vec![root],
                sprite: (sprite, sr.color, sr.sorting_order),
                layer: sr.sorting_layer_id,
                game_layer: prefab.objects[0].layer as i32,
                material: sr.materials.first().and_then(|&m| db.resolve(file, m)),
                id: super::spawn::level1_id(db, r),
                draw: ([sr.flip_x, sr.flip_y], sr.draw_mode, sr.size, sr.adaptive_mode_threshold, sr.sprite_tile_mode == 1),
                visible: true,
                enabled: sr.enabled,
                bounds: super::link::renderer_bounds(&db.sprite(sprite)?, sr),
            });
            self.renderer_sync(self.renderers.len() - 1);
            let mut label = None;
            for o in 1..prefab.objects.len() {
                for (c, script) in prefab.objects[o].components.iter() {
                    let Component::Script { class, enabled, fields: v, .. } = script else { continue };
                    match class.as_str() {
                        "TextMeshPro" => {
                            if prefab.objects[o].parent != Some(0) || !prefab.objects[o].active || !prefab.objects[0].active {
                                unimplemented!("mod-only: a CenterOfGravity text that is not an active child of the marker");
                            }
                            let chain = vec![trs(db, prefab.objects[o].components[0].0)?, root];
                            let text = self.add_world_text(db, file, &prefab, o, v, *enabled, super::spawn::level1_id(db, *c), (o, super::spawn::level1_id(db, prefab.objects[0].components[0].0)), super::BareAnchor::Root, chain, true)?;
                            let appear = prefab.objects[o].components.iter().find_map(|(_, x)| match x {
                                Component::Script { class, enabled: true, fields, .. } if class == "AppearWhenMouseNear" => Some(fields),
                                _ => None,
                            });
                            let Some(a) = appear else { unimplemented!("mod-only: a CenterOfGravity text without a running AppearWhenMouseNear") };
                            // AppearWhenMouseNear.Awake: camera = Camera.main; Center = base.transform when null.
                            let pptr = |name: &str| a.get(name).and_then(Value::pptr).and_then(|p| db.resolve(file, p));
                            if pptr("Center").is_some_and(|t| t != prefab.objects[0].components[0].0) || pptr("Center").is_none() {
                                unimplemented!("mod-only: AppearWhenMouseNear.Center other than its marker's Transform");
                            }
                            let mesh = prefab.objects[o].components.iter().find(|(_, x)| matches!(x, Component::Other(crate::assets::components::MeshRenderer::CLASS))).map(|(r, _)| *r);
                            if pptr("Renderer") != mesh {
                                unimplemented!("mod-only: AppearWhenMouseNear.Renderer other than its text's MeshRenderer");
                            }
                            let float = |name: &str| a.get(name).and_then(Value::f32).ok_or_else(|| format!("AppearWhenMouseNear.{name}"));
                            let consider = a.get("ConsiderParentRenderers").and_then(Value::bool).ok_or("AppearWhenMouseNear.ConsiderParentRenderers")?;
                            label = Some(AppearWhenMouseNear { MinDistance: float("MinDistance")?, ConsiderParentRenderers: consider, text });
                        }
                        "AppearWhenMouseNear" => {}
                        _ => unimplemented!("mod-only: {class} below a CenterOfGravity"),
                    }
                }
            }
            let SizeMultiplier = fields.get("SizeMultiplier").and_then(Value::f32).ok_or("CenterOfGravity.SizeMultiplier")?;
            self.centers.push(Center { bounds: *bounds, SizeMultiplier, renderer, label });
        }
        Ok(())
    }

    // AppearWhenMouseNear.Start: parentRenderers = GetComponentsInParent<Renderer>(), the text's own MeshRenderer and the marker's SpriteRenderer.
    /// CenterOfGravity.Update for every marker, then each AppearWhenMouseNear.Update (level1's file order).
    pub fn centers_update(&mut self) {
        let Some(tool) = self.tool.as_ref() else { return };
        let selected: Vec<usize> = tool.rawSelection.clone();
        for i in 0..self.centers.len() {
            let c = self.centers[i].clone();
            // shouldExist = SelectedObjects.Any(c => c)
            let should_exist = selected.iter().any(|&p| !self.physicals[p].destroyed);
            self.renderers[c.renderer].enabled = should_exist;
            self.renderer_sync(c.renderer);
            if should_exist {
                let position = if c.bounds { self.center_of_bounds_position(&selected) } else { self.center_of_gravity_position(&selected) };
                self.renderers[c.renderer].chain[0].t = [position[0], position[1], position[2], 0.0];
            }
            // camera.orthographicSize * SizeMultiplier * Vector3.one
            let s = self.camera_orthographic_size() * c.SizeMultiplier;
            self.renderers[c.renderer].chain[0].s = [s * 1.0, s * 1.0, s * 1.0, 0.0];
            if let Some(l) = &c.label {
                let root = self.renderers[c.renderer].chain[0];
                *self.world_texts[l.text].chain.last_mut().unwrap() = root;
            }
        }
        for i in 0..self.centers.len() {
            let c = self.centers[i].clone();
            let Some(l) = c.label else { continue };
            self.appear_when_mouse_near_update(&l, c.renderer);
        }
    }

    /// AppearWhenMouseNear.Update; `marker` is the parent SpriteRenderer. A headless run has no Input.mousePosition
    /// and draws nothing, so the distance test waits for a window.
    fn appear_when_mouse_near_update(&mut self, l: &AppearWhenMouseNear, marker: usize) {
        if l.ConsiderParentRenderers && !self.renderers[marker].enabled {
            self.world_texts[l.text].renderer_enabled = false;
            return;
        }
        let Some(screen) = self.input.screen else { return };
        let position = self.renderers[marker].chain[0].t;
        // camera.WorldToScreenPoint(new Vector3(position.x, position.y, camera.transform.position.z)), as a Vector2.
        let b = self.world_to_screen_point([position[0], position[1], 0.0]);
        // Vector2.Distance(Input.mousePosition, b) < MinDistance
        let (dx, dy) = (screen.mouse[0] - b[0], screen.mouse[1] - b[1]);
        let flag = ((d(dx) * dx + d(dy) * dy).0.sqrt() as f32) < l.MinDistance;
        self.world_texts[l.text].renderer_enabled = flag;
    }

    /// CenterOfGravity.GetPosition
    pub(crate) fn center_of_gravity_position(&self, phys: &[usize]) -> [f32; 3] {
        let mut vector = [0.0f32; 3];
        let mut num = 0.0f32;
        for &p in phys {
            if !self.physicals[p].destroyed {
                num += self.physicals[p].rigidbody.mass(&self.world);
            }
        }
        for &p in phys {
            if self.physicals[p].destroyed {
                continue;
            }
            let mass = self.physicals[p].rigidbody.mass(&self.world);
            // RigidbodyType2D.Dynamic is 0.
            let w = if self.physicals[p].rigidbody.bodyType != 0 {
                let pos = self.physicals[p].transform.position;
                [pos[0] * mass, pos[1] * mass, pos[2] * mass]
            } else {
                // localToWorldMatrix.MultiplyPoint3x4(centerOfMass / lossyScale): Vector2 / Vector2 (z dropped), then z 0.
                // Rigidbody2D::GetCenterOfMass (180c1f6b0): zero without a b2 body (a detached Rigidbody2D).
                let rb = &self.physicals[p].rigidbody;
                let com = if rb.detached.is_some() { crate::b2::math::Vec2::new(0.0, 0.0) } else { self.world.body(rb.body).sweep.local_center };
                let ls = self.lossy_scale(p);
                let v = [com.x / ls[0], com.y / ls[1], 0.0];
                let nodes: Vec<Trs> = self.local_chain(p).into_iter().rev().collect();
                let m = crate::unity::transform::local_to_world(&nodes);
                let at = |r: usize| (d(m[r]) * v[0] + d(m[4 + r]) * v[1] + d(m[8 + r]) * v[2] + d(m[12 + r])).f();
                [at(0) * mass, at(1) * mass, at(2) * mass]
            };
            vector = [vector[0] + w[0], vector[1] + w[1], vector[2] + w[2]];
        }
        let nan = |f: f32| super::physical::nan_fallback(f, 0.0);
        [nan(vector[0] / num), nan(vector[1] / num), nan(vector[2] / num)]
    }

    /// CenterOfBounds.GetPosition
    pub(crate) fn center_of_bounds_position(&self, phys: &[usize]) -> [f32; 3] {
        let (mut num, mut num2, mut num3, mut num4) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for &p in phys {
            if self.physicals[p].destroyed || self.physicals[p].sprite.is_none() {
                continue;
            }
            let (c, e) = self.sprite_world_bounds(p);
            let (min, max) = ([c[0] - e[0], c[1] - e[1]], [c[0] + e[0], c[1] + e[1]]);
            if min[0] < num {
                num = min[0];
            }
            if min[1] < num3 {
                num3 = min[1];
            }
            if max[0] > num2 {
                num2 = max[0];
            }
            if max[1] > num4 {
                num4 = max[1];
            }
        }
        [(num + num2) / 2.0, (num3 + num4) / 2.0, 0.0]
    }
}
