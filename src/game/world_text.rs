//! World-space TextMeshPro components (TMPro.TextMeshPro and its MeshRenderer): the properties scripts set, and the
//! mesh OnPreRenderObject regenerates when they changed (crate::tmp::layout). The renderer side draws `mesh`.
#![allow(non_snake_case)]

use std::sync::Arc;

use super::{BareAnchor, Game};
use crate::tmp::{FontAsset, TextMesh, TextProps};
use crate::unity::transform::Trs;

#[derive(Clone, Debug)]
pub struct WorldText {
    pub anchor: BareAnchor,
    pub chain: Vec<Trs>,
    /// The TextMeshPro component's instance id.
    pub id: i32,
    /// Its object in the prefab and the clone's root Transform id, to find the Transforms `chain` copies.
    pub object: usize,
    pub root: i32,
    /// Its GameObject's layer; the MeshRenderer's sortingLayerID and sortingOrder.
    pub game_layer: i32,
    pub layer: i32,
    pub order: i16,
    /// GameObject.activeInHierarchy, Behaviour.enabled (TextMeshPro) and Renderer.enabled (MeshRenderer).
    pub visible: bool,
    pub enabled: bool,
    pub renderer_enabled: bool,
    /// None when the run has no game data to read the font asset from (headless replays): the text keeps its
    /// properties for the scripts and draws nothing.
    pub props: Option<TextProps>,
    /// The text while there are no props (scripts read `text` back).
    pub text: String,
    /// m_havePropertiesChanged
    pub dirty: bool,
    pub mesh: TextMesh,
    /// The TMP_SpriteAnimator component the first `<sprite anim=...>` added (its instance id).
    pub animator: Option<i32>,
    /// A sprite animation rewrote the mesh since it was last drawn.
    pub geometry_changed: bool,
    /// textInfo.meshInfo as the last GenerateTextMesh left it (the arrays a sprite animation writes and uploads), kept while
    /// animations run.
    pub mesh_info: Option<TextMesh>,
    /// TextMeshPro.Awake ran (its m_mesh took an instance id): at Instantiate when active, else at its first activation.
    pub awoken: bool,
    /// The instance ids TextMeshPro.Awake takes: the MeshFilter it adds (when the object has none) and the mesh.
    pub awake_ids: usize,
}

impl WorldText {
    pub fn new(anchor: BareAnchor, chain: Vec<Trs>, id: i32, at: (usize, i32), layers: (i32, i32, i16), active: (bool, bool, bool), text: String, props: Option<TextProps>) -> Self {
        Self { anchor, chain, id, object: at.0, root: at.1, game_layer: layers.0, layer: layers.1, order: layers.2, visible: active.0, enabled: active.1, renderer_enabled: active.2, props, text, dirty: true, mesh: TextMesh::default(), animator: None, geometry_changed: false, mesh_info: None, awoken: active.0, awake_ids: 1 }
    }
}

impl Game {
    /// TMP_Text.text's setter: a different string marks the text for regeneration (SetVerticesDirty).
    pub fn text_set(&mut self, i: usize, value: &str) {
        let t = &mut self.world_texts[i];
        if t.text == value {
            return;
        }
        t.text = value.to_string();
        if let Some(p) = &mut t.props {
            p.text = t.text.clone();
        }
        t.dirty = true;
    }

    /// A TextMeshPro field a script reads without a null check: a null one throws NullReferenceException.
    pub fn text_ref(t: Option<usize>) -> usize {
        t.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
    }

    /// `field.text = value` on a script's TextMeshPro field.
    pub fn text_write(&mut self, t: Option<usize>, value: &str) {
        self.text_set(Self::text_ref(t), value);
    }

    /// Transform.rotation = Quaternion.identity on the text's Transform: its local rotation becomes the inverse of its
    /// parents' (the position is unchanged).
    pub fn text_set_rotation_identity(&mut self, i: usize) {
        use crate::unity::transform::{inverse, normalize};
        let t = &self.world_texts[i];
        let parents = self.anchored_nodes(t.anchor, &t.chain[1..]);
        let mut q = [0.0, 0.0, 0.0, 1.0];
        for n in parents.iter().rev() {
            q = inverse(n, [0.0; 4], q).1;
        }
        let q = normalize(q);
        self.world_texts[i].chain[0].q = q;
        // The clone object's own Transform too (its local rotation is what the game's Transform holds); during its
        // clone's Awake/OnEnable (the clone is not listed yet) awake_behaviours writes it at the end.
        let id = self.world_texts[i].id;
        let owns = |x: &super::spawn::Instance| x.objects.iter().any(|x| x.components.contains(&id));
        if self.unsynced.iter().chain(self.retained.iter()).any(owns) {
            self.with_component_instance(id, |_, inst, o| inst.set_local_rotation(o, q));
        } else {
            self.text_rotation_pending.push((id, q));
        }
    }

    /// Transform.localScale = Vector3.one on the text's Transform.
    pub fn text_set_local_scale_one(&mut self, i: usize) {
        self.world_texts[i].chain[0].s = [1.0, 1.0, 1.0, 0.0];
    }

    /// Transform.localScale of object `o` of the clone `root` changed to `s`: the copies of it in its texts' chains.
    pub fn texts_local_scale_changed(&mut self, inst: &crate::game::spawn::Instance, o: usize) {
        let root = inst.objects[0].components[0];
        for t in self.world_texts.iter_mut().filter(|t| t.root == root) {
            let mut at = Some(t.object);
            for trs in t.chain.iter_mut() {
                let Some(x) = at else { break };
                if x == o {
                    trs.s = inst.objects[o].trs.s;
                }
                at = inst.objects[x].parent;
            }
        }
    }

    /// InputSystem.GetDisplayText: InputAction.GetDisplayText of the named action, "?" for none.
    pub fn input_display_text(&self, name: &str) -> String {
        let defs = &*crate::ui::tables::ACTIONS;
        let Some(i) = defs.iter().position(|a| a.id == name) else { return "?".to_string() };
        let (key, secondary) = match &self.control_binds {
            Some(b) => b[i].clone(),
            None => (defs[i].key.to_string(), defs[i].secondary.to_string()),
        };
        let show = |k: &str| match k {
            "Mouse0" => crate::il!(str "InputAction::KeyToString" 0).to_string(),
            "Mouse1" => crate::il!(str "InputAction::KeyToString" 1).to_string(),
            "Mouse2" => crate::il!(str "InputAction::KeyToString" 2).to_string(),
            k => k.to_string(),
        };
        if secondary != "None" {
            format!("{}+{}", show(&key), show(&secondary))
        } else {
            show(&key)
        }
    }

    /// TextMeshProControlReplacer.Start
    pub fn control_replacer_start(&mut self, i: usize) {
        let (text, pairs) = self.control_replacers[i].clone();
        let t = Self::text_ref(text);
        let mut s = self.text_get(t).to_string();
        for (from, name) in pairs {
            if from.is_empty() {
                super::menu::throw("ArgumentException: String cannot be of zero length.
Parameter name: oldValue");
            }
            s = s.replace(&from, &self.input_display_text(&name));
        }
        self.text_set(t, &s);
    }

    /// TMP_Text.text's getter.
    pub fn text_get(&self, i: usize) -> &str {
        &self.world_texts[i].text
    }

    /// TMP_Text.color's setter (m_fontColor; m_havePropertiesChanged only when it differs).
    pub fn text_color_set(&mut self, i: usize, c: [f32; 4]) {
        let t = &mut self.world_texts[i];
        let Some(p) = &mut t.props else { return };
        if p.font_color != c {
            p.font_color = c;
            t.dirty = true;
        }
    }

    /// TMP_Text.fontSize's setter.
    pub fn text_size_set(&mut self, i: usize, size: f32) {
        let t = &mut self.world_texts[i];
        let Some(p) = &mut t.props else { return };
        if p.font_size != size {
            p.font_size = size;
            p.font_size_base = size;
            t.dirty = true;
        }
    }

    /// Behaviour.enabled of the TextMeshPro (OnEnable sets the vertices dirty).
    pub fn text_enabled_set(&mut self, i: usize, on: bool) {
        let t = &mut self.world_texts[i];
        if t.enabled != on {
            t.enabled = on;
            t.dirty |= on;
        }
    }

    /// Renderer.enabled of its MeshRenderer.
    pub fn text_renderer_enabled_set(&mut self, i: usize, on: bool) {
        self.world_texts[i].renderer_enabled = on;
    }

    /// TextMeshPro.OnPreRenderObject: regenerate a changed, active text (TMP_Text.ForceMeshUpdate does the same). An exception
    /// in ParseInputText (a `<sprite=-2>`) leaves the old mesh and the text dirty, so it throws again every frame; one in
    /// GenerateTextMesh leaves the old mesh drawn and the text no longer dirty, with the animations it started running on
    /// its half-filled textInfo. Unity logs it.
    pub fn text_update_mesh(&mut self, i: usize) -> &TextMesh {
        let t = &self.world_texts[i];
        if t.dirty && t.visible && t.enabled {
            let (mut parsed, mut out, mut partial) = (false, None, None);
            let caught = t.props.as_ref().and_then(|p| super::menu::catch_managed(|| out = Some(crate::tmp::layout::generate_with(p, &mut parsed, &mut partial))));
            if t.props.is_none() || parsed {
                self.world_texts[i].dirty = false;
            }
            let info = match out {
                Some(mesh) => {
                    self.world_texts[i].mesh = mesh.clone();
                    Some(mesh)
                }
                None => partial,
            };
            // Only a GenerateTextMesh that got past its StopAllAnimations has animations; otherwise the old ones go on.
            if let Some(info) = info.filter(|m| m.sprite_anims.is_some()) {
                let anims = info.sprite_anims.clone().unwrap();
                self.world_texts[i].mesh_info = (!anims.is_empty()).then_some(info);
                self.text_sprite_animations(i, anims);
            }
            self.log_exception(caught);
        }
        &self.world_texts[i].mesh
    }

    /// GenerateTextMesh's m_spriteAnimator.StopAllAnimations, then DoSpriteAnimation for each `<sprite anim=start,end,framerate>`
    /// it parsed (the `spriteAnimator` getter's GetComponent / AddComponent<TMP_SpriteAnimator> on the first one).
    fn text_sprite_animations(&mut self, i: usize, anims: Vec<crate::tmp::SpriteAnim>) {
        if let Some(a) = self.world_texts[i].animator {
            self.stop_all_coroutines(a);
        }
        if anims.is_empty() {
            return;
        }
        let owner = match self.world_texts[i].animator {
            Some(a) => a,
            None => {
                let a = self.allocate_instance_id();
                self.add_script_to_clone(self.world_texts[i].id, "TMP_SpriteAnimator", a);
                self.world_texts[i].animator = Some(a);
                a
            }
        };
        for a in anims {
            self.co_start(owner, Self::sprite_animation_routine(i, a));
        }
    }

    /// TMP_SpriteAnimator.DoSpriteAnimationInternal: from the next frame, every 1/|framerate| s the character's quad and uvs
    /// show the next sprite of start..=end (backwards for a negative framerate), until the character is an ellipsis or ETX.
    fn sprite_animation_routine(i: usize, a: crate::tmp::SpriteAnim) -> super::coroutine::Routine {
        use super::coroutine::Yield;
        let mut step = 0;
        let (mut frame, mut end, mut base_scale, mut elapsed, mut target) = (a.start, a.end, 0.0f32, 0.0f32, 0.0f32);
        Box::new(move |g| {
            step += 1;
            // `yield return null` (m_TextComponent is the text).
            if step == 1 {
                return Yield::Null;
            }
            let sa = &a.asset;
            if step == 2 {
                if end > sa.characters.len() as i32 {
                    end = sa.characters.len() as i32 - 1;
                }
                if a.info.is_none() {
                    super::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array.");
                }
                let first = crate::tmp::layout::sprite_character(sa, a.start);
                base_scale = first.scale * first.glyph.scale;
                // Mathf.Abs(int)
                if a.framerate == i32::MIN {
                    super::menu::throw("OverflowException: Negating the minimum value of a twos complement number is invalid.");
                }
                target = 1.0 / a.framerate.abs() as f32;
            } else if elapsed > target {
                elapsed = 0.0;
                let info = a.info.unwrap();
                if info.character == 3 || info.character == 0x2026 {
                    return Yield::Done;
                }
                let sc = crate::tmp::layout::sprite_character(sa, frame);
                let num = info.scale / base_scale * sc.scale * sc.glyph.scale;
                let m = sc.glyph.metrics;
                let v2 = [info.origin + m.horizontal_bearing_x * num, info.baseline + (m.horizontal_bearing_y - m.height) * num, 0.0];
                let v3 = [v2[0], info.baseline + m.horizontal_bearing_y * num, 0.0];
                let v4 = [info.origin + (m.horizontal_bearing_x + m.width) * num, v3[1], 0.0];
                let v5 = [v4[0], v2[1], 0.0];
                let r = sc.glyph.rect;
                let (w, h) = (sa.sheet_size[0] as f32, sa.sheet_size[1] as f32);
                let uv6 = [r[0] as f32 / w, r[1] as f32 / h];
                let uv7 = [uv6[0], (r[1] + r[3]) as f32 / h];
                let uv8 = [(r[0] + r[2]) as f32 / w, uv7[1]];
                let uv9 = [uv8[0], uv6[1]];
                g.text_set_quad(i, info.mat, info.quad, [v2, v3, v4, v5], [uv6, uv7, uv8, uv9]);
                frame = if a.framerate > 0 {
                    if frame >= end { a.start } else { frame + 1 }
                } else if frame <= a.start {
                    end
                } else {
                    frame - 1
                };
            }
            elapsed += g.time.deltaTime;
            Yield::Null
        })
    }

    /// meshInfo[mat].vertices and uvs0 of quad `q`, then `meshInfo.mesh.vertices = vertices; mesh.uv = uvs`: the material's
    /// drawn quads become meshInfo's (past the filled quads TMP's arrays hold zeroed vertices: empty quads here).
    fn text_set_quad(&mut self, i: usize, mat: usize, q: usize, pos: [[f32; 3]; 4], uv: [[f32; 2]; 4]) {
        let t = &mut self.world_texts[i];
        let Some(info) = t.mesh_info.as_mut() else { return };
        let mut quads = material_quads(info, mat);
        if quads.len() <= q {
            quads.resize(q + 1, Default::default());
        }
        (quads[q].pos, quads[q].uv) = (pos, uv);
        let sub = info.submeshes.get(mat.wrapping_sub(1)).cloned();
        set_material_quads(info, mat, sub.clone(), quads.clone());
        set_material_quads(&mut t.mesh, mat, sub, quads);
        t.geometry_changed = true;
    }

    /// The TextMeshPro (fields `v`) on object `o` of `prefab` with its RectTransform and MeshRenderer, into
    /// `world_texts`; returns its index.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn add_world_text(&mut self, db: &mut crate::assets::db::AssetDb, file: usize, prefab: &crate::assets::db::Prefab, o: usize, v: &crate::assets::mono::Value, enabled: bool, id: i32, at: (usize, i32), anchor: BareAnchor, chain: Vec<Trs>, active: bool) -> Result<usize, String> {
        use crate::assets::db::Component;
        use crate::assets::mono::Value;
        let mesh_renderer = prefab.objects[o].components.iter().find(|(_, x)| matches!(x, Component::Other(crate::assets::components::MeshRenderer::CLASS))).map(|(r, _)| crate::assets::components::MeshRenderer::read(db.data(*r))).transpose()?.ok_or("TextMeshPro without its MeshRenderer")?;
        let rect = crate::assets::classes::RectTransform::read(db.data(prefab.objects[o].components[0].0))?;
        let parent_rect = prefab.objects[o].parent.is_some_and(|p| db.class(prefab.objects[p].components[0].0) == 224);
        if parent_rect && rect.anchor_min != rect.anchor_max {
            unimplemented!("mod-only: {}: a TextMeshPro stretched by its parent RectTransform", prefab.objects[o].name);
        }
        // RectTransform.rect: sizeDelta, as the parent's rect does not stretch it.
        let text = match v.get("m_text") {
            Some(Value::Str(s)) => s.clone(),
            _ => return Err("TextMeshPro.m_text".into()),
        };
        let font_ref = v.get("m_fontAsset").and_then(Value::pptr).and_then(|p| db.resolve(file, p));
        let font = self.tmp_font(db, font_ref.unwrap_or_else(|| unimplemented!("mod-only: {}: TextMeshPro without a font asset (TMP_Settings.defaultFontAsset)", prefab.objects[o].name)))?;
        let shared = v.get("m_sharedMaterial").and_then(Value::pptr).and_then(|p| db.resolve(file, p));
        let material = if shared.is_none() || shared == font.material.material {
            font.material.clone()
        } else {
            let m = shared.unwrap();
            let (mat, keywords) = crate::assets::components::Material::read_with_keywords(db.data(m))?;
            crate::tmp::SdfMaterial { material: Some(m), shader: crate::tmp::material::shader_name(db.data(db.resolve(m.file, mat.shader).ok_or("the text material shader")?)), floats: mat.floats.into_iter().collect(), colors: mat.colors.into_iter().collect(), keywords: keywords.split_whitespace().map(str::to_string).collect() }
        };
        let props = Some(crate::tmp::TextProps::read(v, font, material, rect.size_delta, rect.pivot)?);
        let layers = (prefab.objects[o].layer as i32, mesh_renderer.sorting_layer_id, mesh_renderer.sorting_order);
        // TextMeshPro.Awake takes instance ids: AddComponent<MeshFilter>() when the object has none, then m_mesh = new Mesh()
        // (GaugeBehaviour.CreateLabels' labels, 5 objects without a MeshFilter, are 14 ids apart in the game).
        // (One awoken later, at its first SetActive(true), takes them then: awake_behaviours' TextMeshPro arm.)
        let awake_ids = if prefab.objects[o].components.iter().any(|(_, x)| matches!(x, Component::Other(33))) { 1 } else { 2 };
        if active {
            self.allocate_instance_ids(awake_ids);
        }
        self.world_texts.push(WorldText { awake_ids, ..WorldText::new(anchor, chain, id, at, layers, (active, enabled, mesh_renderer.enabled), text, props) });
        Ok(self.world_texts.len() - 1)
    }

    /// The font asset at `r`, read once per run (TMP_FontAsset is shared by every text using it).
    pub fn tmp_font(&mut self, db: &mut crate::assets::db::AssetDb, r: crate::assets::db::ObjRef) -> Result<Arc<FontAsset>, String> {
        FontAsset::read(db, r, &mut self.tmp_fonts)
    }
}

/// Material `mat`'s quads of a text mesh.
fn material_quads(m: &TextMesh, mat: usize) -> Vec<crate::tmp::Quad> {
    if mat == 0 {
        m.quads[..m.main_len].to_vec()
    } else {
        m.submeshes.get(mat - 1).map_or(Vec::new(), |s| s.quads.clone())
    }
}

/// Replace material `mat`'s quads (a sub mesh the text does not have yet comes from `sub`), keeping `quads` flattened.
fn set_material_quads(m: &mut TextMesh, mat: usize, sub: Option<crate::tmp::SubMesh>, quads: Vec<crate::tmp::Quad>) {
    let mut main = m.quads[..m.main_len].to_vec();
    if mat == 0 {
        main = quads;
    } else {
        while m.submeshes.len() < mat {
            let mut s = sub.clone().expect("the sub mesh meshInfo has");
            s.quads.clear();
            m.submeshes.push(s);
        }
        m.submeshes[mat - 1].quads = quads;
    }
    m.main_len = main.len();
    m.quads = main.into_iter().chain(m.submeshes.iter().flat_map(|s| s.quads.iter().copied())).collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_game(text: &str) -> Option<Game> {
        let Ok(game) = crate::game_assets::find_game(None) else {
            // PVCPC sets PPG_GAME_DIR: there these tests must run.
            assert!(std::env::var_os("PPG_GAME_DIR").is_none(), "PPG_GAME_DIR is set but names no game");
            return None;
        };
        let mut db = crate::assets::db::AssetDb::open(&game).unwrap();
        let mut cache = std::collections::HashMap::new();
        let s = crate::tmp::TmpSettings::read(&mut db, &mut cache).unwrap();
        let f = s.default_font.clone().expect("TMP Settings' default font");
        let props = TextProps { text: text.into(), font: f, font_size: 1.0, font_size_base: 1.0, font_size_max: 1.0, font_size_min: 0.1, rect_size: [20.0, 5.0], pivot: [0.5, 0.5], lossy_scale_y: 1.0, is_rich_text: true, settings: s, ..Default::default() };
        let mut g = Game::default();
        g.world_texts.push(WorldText::new(BareAnchor::Root, Vec::new(), 1, (0, 1), (0, 0, 0), (true, true, true), text.into(), Some(props)));
        Some(g)
    }

    /// One 0.05 s frame (the coroutine tests' frame without fixed steps).
    fn step(g: &mut Game) {
        g.time.deltaTime = 0.05;
        g.time.frameCount += 1;
        g.time.dynamicTimeAsDouble += 0.05;
        g.time.timeAsDouble = g.time.dynamicTimeAsDouble;
        g.gore_string_delayed_calls(true);
        g.end_of_frame_calls();
    }

    /// `<sprite=-2>` passes the tag's range check and throws in ParseInputText: the old mesh stays and the text stays dirty.
    #[test]
    fn a_negative_sprite_index_throws_every_frame() {
        let Some(mut g) = text_game("a<sprite=-2>") else { return };
        g.text_update_mesh(0);
        g.text_update_mesh(0);
        assert!(g.world_texts[0].dirty && g.world_texts[0].mesh.quads.is_empty());
    }

    /// `<sprite anim=0,2,10>`: the animator is added, and from the frame after next the sprite's uvs step through the sprites.
    #[test]
    fn an_animated_sprite_steps_its_frames() {
        let Some(mut g) = text_game("<sprite anim=0,2,10>") else { return };
        g.text_update_mesh(0);
        let t = &g.world_texts[0];
        assert!(!t.dirty && t.animator.is_some());
        assert_eq!(t.mesh.sprite_anims.as_ref().map(|a| a.len()), Some(1));
        let first = t.mesh.quads[0].uv;
        let mut seen = vec![first];
        for _ in 0..12 {
            step(&mut g);
            let uv = g.world_texts[0].mesh.quads[0].uv;
            if *seen.last().unwrap() != uv {
                seen.push(uv);
            }
        }
        assert!(seen.len() >= 3, "{seen:?}");
        assert!(g.world_texts[0].geometry_changed);
        // A new text stops the animation (GenerateTextMesh's StopAllAnimations).
        g.text_set(0, "b");
        g.text_update_mesh(0);
        g.world_texts[0].geometry_changed = false;
        for _ in 0..6 {
            step(&mut g);
        }
        assert!(!g.world_texts[0].geometry_changed);
    }

    /// A GenerateTextMesh that throws after a `<sprite anim>` started (the ellipsis' end-of-text U+0003 missing from the font:
    /// KeyNotFoundException) leaves the old mesh drawn and the text clean; the animation runs on the half-built textInfo and
    /// uploads its arrays.
    #[test]
    fn a_throw_after_an_animation_leaves_it_running() {
        let Some(mut g) = text_game("<sprite anim=0,2,10> HELLO WORLD AGAIN AND AGAIN") else { return };
        let p = g.world_texts[0].props.as_mut().unwrap();
        let mut f = (*p.font).clone();
        assert!(f.characters.contains_key(&8230), "the default font has U+2026");
        f.characters.remove(&3);
        p.font = std::sync::Arc::new(f);
        (p.font_size, p.font_size_base, p.font_size_max) = (0.8, 0.8, 0.8);
        (p.rect_size, p.overflow_mode, p.enable_word_wrapping) = ([0.5, 0.1], 1, true);
        g.text_update_mesh(0);
        let t = &g.world_texts[0];
        assert!(!t.dirty && t.animator.is_some() && t.mesh.quads.is_empty(), "{} {:?} {}", t.dirty, t.animator, t.mesh.quads.len());
        assert_eq!(t.mesh_info.as_ref().and_then(|m| m.sprite_anims.as_ref()).map(|a| a.len()), Some(1));
        for _ in 0..4 {
            step(&mut g);
        }
        assert!(g.world_texts[0].geometry_changed && !g.world_texts[0].mesh.quads.is_empty());
    }
}
