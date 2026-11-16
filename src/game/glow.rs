//! GlowInTheDarkBehaviour, which Tritium adds to the containers it enters: its ModLightPrefab light under the container.
#![allow(non_snake_case)]

use super::container::Owner;
use super::visual::SpriteAt;
use super::{mathf, script, Game};
use crate::assets::db::{Component, ObjRef};
use crate::unity::transform::Trs;

#[derive(Clone, Copy, Debug)]
pub struct GlowInTheDark {
    /// The component's instance id.
    pub id: i32,
    /// The BloodContainer on its GameObject.
    pub owner: Owner,
    /// Start's lightSprite: the ModLightPrefab SpriteRenderer's `Game::renderers` entry.
    pub light: Option<usize>,
}

script! {
    /// LightSprite (ModLightPrefab's root): its colour, brightness and radius as last set.
    LightSprite {
        color: [f32; 4] = [0.0; 4],
        brightness: f32 = 0.0,
        radius: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// SpriteRenderer (not under its field's name: a recorded `@name#id` would parse over it).
        renderer: Option<SpriteAt> = None,
        /// A recording's SpriteRenderer instance id (its renderer is found when called: it may load after the script).
        renderer_id: i32 = 0,
        enabled: bool = true,
    }
}

impl Game {
    fn light_sprite_renderer(&self, l: usize) -> Option<SpriteAt> {
        let x = &self.light_sprites[l];
        x.renderer.or_else(|| (x.renderer_id != 0).then(|| self.item_sprite(x.renderer_id)).flatten())
    }

    /// LightSprite.Color's setter: color, SpriteRenderer.color.
    pub fn light_sprite_set_color(&mut self, l: usize, c: [f32; 4]) {
        self.light_sprites[l].color = c;
        let at = self.light_sprite_renderer(l).unwrap_or_else(|| super::menu::throw("NullReferenceException: LightSprite.SpriteRenderer"));
        self.set_sprite_color(at, c);
    }

    /// LightSprite.Brightness's setter: brightness, _GlowIntensity in its block (Awake's empty one plus earlier sets:
    /// only _GlowIntensity), SetPropertyBlock.
    pub fn light_sprite_set_brightness(&mut self, l: usize, v: f32) {
        self.light_sprites[l].brightness = v;
        let at = self.light_sprite_renderer(l).unwrap_or_else(|| super::menu::throw("NullReferenceException: LightSprite.SpriteRenderer"));
        let mut block = super::props::PropBlock::default();
        block.set_float("_GlowIntensity", v);
        self.set_property_block(at, &block);
    }

    /// LightSprite.Radius's setter: radius, transform.localScale = Vector3.one * radius (its object is the renderer's).
    pub fn light_sprite_set_radius(&mut self, l: usize, v: f32) {
        self.light_sprites[l].radius = v;
        if let Some(SpriteAt::Renderer(r)) = self.light_sprite_renderer(l) {
            self.renderers[r].chain[0].s = [v, v, v, 0.0];
            self.renderer_object(r, |inst, o| inst.set_local_scale(o, [v, v, v]));

            self.renderer_sync(r);
        }
    }

    /// Tritium.OnEnterContainer: container.gameObject.GetOrAddComponent<GlowInTheDarkBehaviour>().
    pub fn tritium_on_enter_container(&mut self, _this: usize, o: Owner) {
        // A destroyed component stays until its delayed Destroy runs.
        if self.glows.iter().any(|g| g.owner == o && !self.destroyed_components.contains(&g.id)) {
            return;
        }
        let id = self.allocate_instance_id();
        self.glows.push(GlowInTheDark { id, owner: o, light: None });
        self.pending_starts.push(super::misc::PendingStart::Glow(self.glows.len() - 1));
    }

    /// Tritium.OnUpdate
    pub(crate) fn tritium_on_update(&mut self, o: Owner) {
        let Owner::Circulation(l) = o else { return };
        if self.limbs[l].SpeciesIdentity == "Android" {
            let limb = &mut self.limbs[l];
            limb.BaseStrength = super::mathf::lerp(limb.BaseStrength, 18.0, 0.5);
            return;
        }
        let p = self.limbs[l].PhysicalBehaviour;
        if self.physicals[p].Temperature < 40.0 {
            self.physicals[p].Temperature += self.random.value();
        }
        if self.limbs[l].RoughClassification == 0 {
            if self.random.value() > 0.995 {
                let damage = self.limbs[l].Health + 1.0;
                self.limb_damage(l, damage);
            }
            // (double)Random.value > 0.9
            if self.random.value() as f64 > 0.9 {
                let pain = self.random.value() * 60.0;
                self.add_pain(self.limbs[l].Person, pain);
            }
            self.people[self.limbs[l].Person].Consciousness *= 0.99;
        }
    }

    /// Tritium.OnEnterLimb
    pub(crate) fn tritium_on_enter_limb(&mut self, l: usize) {
        if self.limbs[l].HasBrain {
            let person = self.limbs[l].Person;
            if self.people[person].Braindead {
                self.limbs[l].Health += 0.1;
                self.people[person].Braindead = false;
            }
            if self.people[person].BrainDamaged {
                self.people[person].BrainDamaged = false;
            }
        }
    }

    /// GlowInTheDarkBehaviour.OnDestroy: Object.Destroy(lightSprite.gameObject) and Object.Destroy(glow) (their end-of-frame
    /// destroys come before the frame is drawn).
    pub fn glow_run_destroy(&mut self, id: i32) -> bool {
        let Some(g) = self.glows.iter().position(|x| x.id == id) else { return false };
        self.component_destroys.retain(|&(_, i)| i != id);
        if self.destroyed_components.insert(id) {
            if let Some(r) = self.glows[g].light {
                self.renderers[r].visible = false;
                self.renderer_sync(r);
            }
        }
        true
    }

    /// Tritium.OnExitContainer: Object.Destroy of its GlowInTheDarkBehaviour (OnDestroy removes the light).
    pub fn tritium_on_exit_container(&mut self, _this: usize, o: Owner) {
        if let Some(g) = self.glows.iter().find(|g| g.owner == o && !self.destroyed_components.contains(&g.id)) {
            let id = g.id;
            self.tr_destroy_component(id, 0.0);
        }
    }

    // LightSprite.Awake: glow_start (its SpriteRenderer; GetPropertyBlock of the new renderer is empty)
    // LightSprite.Color: glow_start (SpriteRenderer.color)
    // LightSprite.Brightness: glow_start (_GlowIntensity in the property block)
    // LightSprite.Radius: glow_start (localScale = Vector3.one * radius)
    /// GlowInTheDarkBehaviour.Start: new GameObject(name, typeof(Optout)) takes the GameObject's, Transform's and
    /// Optout's ids, then ModdingApi.CreateLight instantiates Prefabs/ModLightPrefab under it (LightSprite.Awake only
    /// reads its renderer).
    pub fn glow_start(&mut self, g: usize) {
        if self.destroyed_components.contains(&self.glows[g].id) {
            return;
        }
        for _ in 0..3 {
            self.allocate_instance_id();
        }
        let mut db = self.assets.take().expect("GlowInTheDarkBehaviour without the game's data");
        let prefab = db.resource("Prefabs/ModLightPrefab", 1).ok().flatten().and_then(|r| db.prefab(r).ok()).unwrap_or_else(|| panic!("Resources.Load(Prefabs/ModLightPrefab) found no GameObject"));
        // The clone's ids: each object's GameObject, then its components (instantiate_native's order).
        let mut ids = Vec::new();
        for o in &prefab.objects {
            self.allocate_instance_id();
            ids.push(o.components.iter().map(|_| self.allocate_instance_id()).collect::<Vec<_>>());
        }
        // ModdingApi.CreateLight: GetComponent<LightSprite>() of the clone's root; LightSprite.Awake keeps its
        // SpriteRenderer field, or takes its own.
        let root = &prefab.objects[0];
        let file = root.game_object.file;
        let Some((_, Component::Script { fields, .. })) = root.components.iter().find(|(_, k)| matches!(k, Component::Script { class, .. } if class == "LightSprite")) else {
            unimplemented!("mod-only: ModLightPrefab without a LightSprite on its root (NullReferenceException)")
        };
        let field = fields.get("SpriteRenderer").and_then(|v| v.pptr()).filter(|p| !p.is_null());
        let at = match field {
            Some(p) => *prefab.index.get(&ObjRef { file, path_id: p.path_id }).unwrap_or_else(|| unimplemented!("mod-only: LightSprite.SpriteRenderer outside the prefab")),
            None => (0, root.components.iter().position(|(_, k)| matches!(k, Component::SpriteRenderer(_))).unwrap_or_else(|| unimplemented!("mod-only: LightSprite without a SpriteRenderer (NullReferenceException)"))),
        };
        if at.0 != 0 {
            unimplemented!("mod-only: LightSprite.SpriteRenderer on a child of ModLightPrefab");
        }
        let Component::SpriteRenderer(r) = &root.components[at.1].1 else { unreachable!() };
        let Some((_, Component::Transform(t))) = root.components.first() else { unreachable!() };
        let rotation = t.local_rotation;
        let sprite = db.resolve(file, r.sprite).unwrap_or_else(|| unimplemented!("mod-only: ModLightPrefab's SpriteRenderer without a sprite"));
        let bounds = super::link::renderer_bounds(&crate::assets::components::Sprite::read(db.data(sprite)).expect("the light Sprite"), r);
        let material = r.materials.first().and_then(|&m| db.resolve(file, m));
        let (layer, order, game_layer, enabled) = (r.sorting_layer_id, r.sorting_order, root.layer as i32, r.enabled);
        let draw = ([r.flip_x, r.flip_y], r.draw_mode, r.size, r.adaptive_mode_threshold, r.sprite_tile_mode == 1);
        let id = ids[0][at.1];
        let light_id = root.components.iter().position(|(_, k)| matches!(k, Component::Script { class, .. } if class == "LightSprite")).map(|c| ids[0][c]).unwrap();
        self.assets = Some(db);
        // radius = Mathf.Sqrt(PhysicalBehaviour.ObjectArea) * 3f: every container owner carries one.
        let p = self.container_physical(self.glows[g].owner);
        let radius = mathf::sqrt(self.physicals[p].ObjectArea) * 3.0;
        // glow: SetParent(transform), localPosition and localRotation default (SetLocalTR normalizes the zero
        // quaternion to identity), localScale one. The light: Instantiate(..., parent) keeps the prefab's local
        // rotation; localPosition zero, localScale = Vector3.one * radius.
        let glow = Trs { t: [0.0; 4], q: crate::unity::transform::normalize([0.0; 4]), s: [1.0, 1.0, 1.0, 0.0] };
        let light = Trs { t: [0.0; 4], q: rotation, s: [radius, radius, radius, 0.0] };
        self.glows[g].light = Some(self.renderers.len());
        self.renderers.push(super::Renderer {
            anchor: super::BareAnchor::Physical(p),
            chain: vec![light, glow],
            // LightSprite.Color = new Color(0.08f, 0.62f, 0.28f): SpriteRenderer.color.
            sprite: (sprite, [0.08, 0.62, 0.28, 1.0], order),
            layer,
            game_layer,
            material,
            id,
            draw,
            visible: self.physicals[p].active,
            enabled,
            bounds,
        });
        self.renderer_sync(self.renderers.len() - 1);
        // LightSprite.Brightness = 0.25f: SetFloat(_GlowIntensity) into Awake's (empty) block, SetPropertyBlock.
        let mut block = super::props::PropBlock::default();
        block.set_float("_GlowIntensity", 0.25);
        self.set_property_block(SpriteAt::Renderer(self.renderers.len() - 1), &block);
        let r = self.renderers.len() - 1;
        self.light_sprites.push(LightSprite { id: light_id, renderer: Some(SpriteAt::Renderer(r)), color: [0.08, 0.62, 0.28, 1.0], brightness: 0.25, radius, ..Default::default() });
    }
}
