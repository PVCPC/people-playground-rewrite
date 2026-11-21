//! MapConfig's weather overlays (MapConfig.OverlayState): the Snow, Rain and Fog clones that PrepareWeatherInstance makes,
//! their fade in Update/OverlayFadeStep, their GameObject activity and their AudioSource's volume (the rain's RainLoop).
//! Material._Density goes to the overlay shaders (Unlit/SnowOverlay, Unlit/FogShader: custom_material.rs) of the clones'
//! SpriteRenderers. The fog's PostProcessVolume weight is (lights.rs blends it).
use super::mathf;
use super::spawn::Instance;
use super::Game;
use crate::assets::db::{Component, Prefab};

/// One OverlayState.
#[derive(Debug, Default)]
pub struct Overlay {
    pub inst: Instance,
    pub Target: bool,
    pub Visiblity: f32,
    /// OptionalAudioSource: GetComponentInChildren<AudioSource>(includeInactive: true).
    pub OptionalAudioSource: Option<i32>,
    /// OptionalPostProcessing: GetComponentInChildren<PostProcessVolume>(includeInactive: true), with its object.
    pub OptionalPostProcessing: Option<(usize, super::map::PostVolume)>,
    /// Material: GetComponentInChildren<Renderer>().sharedMaterial, as the `renderers` entry drawing it.
    pub Material: Option<usize>,
    /// The weather slot it was made for (0 Snow, 1 Rain, 2 Fog).
    pub kind: usize,
    /// The clone's AudioSourceTimeScaleBehaviours whose Start has not run (root Transform id, IsAmbience), by object.
    time_scale_starts: Vec<(usize, i32, bool)>,
}

impl Game {
    /// OverlayState..ctor(gameObject): GameObject, Material (the renderer's sharedMaterial: drawing), OptionalPostProcessing (the overlay
    /// shaders' weight: drawing) and OptionalAudioSource = GetComponentInChildren<AudioSource>(includeInactive: true), the first in
    /// the root-first depth-first order. MapConfig.PrepareWeatherInstance's clone is passed after its SetActive(false): its AudioSources
    /// registered (inactive), then, for the rain, `GetComponent<AudioSource>().clip = RainLoop`.
    pub(super) fn map_overlay_add(&mut self, inst: Instance, w: &Prefab, rain_loop: Option<Option<super::audio::ClipRef>>) -> Overlay {
        let db = self.assets.take().expect("MapConfig.PrepareWeatherInstance without the game's data");
        let mut first = None;
        let mut post = None;
        let mut material = None;
        let mut time_scale_starts = Vec::new();
        // GetComponentInChildren: the root first, then its children depth first.
        for o in std::iter::once(0).chain(inst.descendants(0)) {
            for (c, (_, comp)) in w.objects[o].components.iter().enumerate() {
                match comp {
                    Component::AudioSource { source, clip, group } => {
                        let clip = super::audio::clip_ref(&db, &mut self.audio.clip_cache, *clip).expect("weather AudioSource clip");
                        let ancestors = std::iter::successors(Some(o), |&x| inst.objects[x].parent).map(|x| inst.objects[x].components[0]).collect();
                        let s = super::audio::AudioSource::from_component(source, clip, group.as_deref(), super::BareAnchor::Root, inst.chain(o), ancestors, inst.objects[o].active);
                        let id = inst.objects[o].components[c];
                        self.audio.add(id, s);
                        first.get_or_insert(id);
                    }
                    // Drawn like the map's own SpriteRenderers (link.rs): the clone hangs off the map root.
                    Component::SpriteRenderer(r) => {
                        let file = w.objects[o].game_object.file;
                        let sprite = db.resolve(file, r.sprite).expect("weather SpriteRenderer sprite");
                        let bounds = super::link::renderer_bounds(&crate::assets::components::Sprite::read(db.data(sprite)).expect("weather sprite"), r);
                        let renderer = super::Renderer { anchor: super::BareAnchor::Root, chain: inst.chain(o), sprite: (sprite, r.color, r.sorting_order), layer: r.sorting_layer_id, game_layer: w.objects[o].layer as i32, material: r.materials.first().and_then(|&m| db.resolve(file, m)), id: inst.objects[o].components[c], draw: ([r.flip_x, r.flip_y], r.draw_mode, r.size, r.adaptive_mode_threshold, r.sprite_tile_mode == 1), visible: inst.objects[o].active, enabled: r.enabled, bounds };
                        // The shared material's _Density an earlier clone's fade set (the asset keeps it).
                        let density = renderer.material.and_then(|m| self.overlay_densities.get(&m).copied());
                        self.renderers.push(renderer);
                        if let Some(v) = density {
                            self.set_material_prop(super::visual::SpriteAt::Renderer(self.renderers.len() - 1), "_Density", super::props::Prop::Float(v));
                        }
                        material.get_or_insert(self.renderers.len() - 1);
                    }
                    Component::Script { class, .. } if class == "PostProcessVolume" => {
                        if post.is_none() {
                            post = Some((o, super::map::post_volume_of(&db, w, o, c, inst.objects[o].components[c])));
                        }
                    }
                    Component::Script { class, enabled, fields, .. } if class == "AudioSourceTimeScaleBehaviour" && *enabled => {
                        let ambience = fields.get("IsAmbience").and_then(crate::assets::mono::Value::bool).expect("AudioSourceTimeScaleBehaviour.IsAmbience");
                        time_scale_starts.push((o, inst.objects[o].components[0], ambience));
                    }
                    _ => {}
                }
            }
        }
        self.assets = Some(db);
        if let Some(clip) = rain_loop {
            // RainInstance.GetComponent<AudioSource>() on the root; null throws at `.clip`.
            let root = w.objects[0].components.iter().position(|(_, k)| matches!(k, Component::AudioSource { .. }));
            let root = root.unwrap_or_else(|| super::menu::throw("NullReferenceException: the rain prefab has no AudioSource"));
            self.audio.set_clip(inst.objects[0].components[root], clip);
        }
        Overlay { inst, Target: false, Visiblity: 0.0, OptionalAudioSource: first, OptionalPostProcessing: post, Material: material, kind: 0, time_scale_starts }
    }

    /// The map's weather clones go with it (children of MapConfig's Transform): their AudioSources stop, their sprites go.
    pub(super) fn map_overlays_destroy(&mut self) {
        // The MapConfig goes with them: the next map's Start begins from its own Settings.
        (self.map_weather, self.map_lightning, self.map_settings, self.map_config) = ([None; 3], None, None, None);
        for o in std::mem::take(&mut self.map_overlays).into_iter().flatten() {
            for x in &o.inst.objects {
                for &id in &x.components {
                    self.audio.remove(id);
                    if let Some(r) = self.renderers.iter_mut().find(|r| r.id == id) {
                        r.visible = false;
                    }
                }
            }
        }
    }

    /// MapConfig.ApplySettings: overlayTarget[Snow/Rain/FogInstance].Target = settings.Snow / Rain / Fog.
    pub(super) fn map_config_set_overlay_targets(&mut self, settings: &super::map::EnvironmentalSettings) {
        for (k, on) in [settings.Snow, settings.Rain, settings.Fog].into_iter().enumerate() {
            if let Some(Some(o)) = self.map_weather[k].map(|i| &mut self.map_overlays[i]) {
                o.Target = on;
            }
        }
    }

    /// MapConfig.Update: OverlayFadeStep of each overlayTarget entry, in insertion order.
    pub fn map_config_update(&mut self) {
        for k in 0..self.map_overlays.len() {
            if self.map_overlays[k].is_some() {
                self.map_config_overlay_fade_step(k);
            }
        }
    }

    /// MapConfig.OverlayFadeStep
    pub(crate) fn map_config_overlay_fade_step(&mut self, k: usize) {
        let dt = self.time.deltaTime;
        let mut o = self.map_overlays[k].take().unwrap();
        let target = if o.Target { 1.0 } else { 0.0 };
        o.Visiblity = mathf::lerp(o.Visiblity, target, mathf::lerp_factor_dt(0.7, dt));
        // setActive(item.GameObject, item.Visiblity > 0.01f): SetActive only when activeSelf differs.
        let a = o.Visiblity > 0.01;
        self.map_overlay_set_active(&mut o, a);
        if (o.Visiblity - target).abs() > 0.005 {
            // Material.SetFloat("_Density"), then OptionalPostProcessing.weight = Visiblity^2.
            if let Some(r) = o.Material {
                self.overlay_set_density(r, o.Visiblity);
            }
            if let Some((_, v)) = &mut o.OptionalPostProcessing {
                v.weight = o.Visiblity * o.Visiblity;
                if let Some(x) = self.post_volumes.iter_mut().find(|x| x.id == v.id) {
                    x.weight = v.weight;
                }
            }
            if let Some(id) = o.OptionalAudioSource {
                self.audio.get_mut(id).set_volume(o.Visiblity);
            }
        }
        self.map_overlays[k] = Some(o);
    }

    /// OverlayState.Material.SetFloat("_Density", v): Material is the renderer's sharedMaterial, so every renderer drawing it (the
    /// clones of each Reinitialise share it) and the clones made later (the asset keeps the value).
    pub(super) fn overlay_set_density(&mut self, r: usize, v: f32) {
        let Some(m) = self.renderers[r].material else {
            return self.set_material_prop(super::visual::SpriteAt::Renderer(r), "_Density", super::props::Prop::Float(v));
        };
        self.overlay_densities.insert(m, v);
        for i in 0..self.renderers.len() {
            if self.renderers[i].material == Some(m) {
                self.set_material_prop(super::visual::SpriteAt::Renderer(i), "_Density", super::props::Prop::Float(v));
            }
        }
    }

    /// MapConfig.OverlayFadeStep's setActive(item.GameObject, active): SetActive only when activeSelf differs.
    pub(super) fn map_overlay_set_active(&mut self, o: &mut Overlay, a: bool) {
        if o.inst.objects[0].self_active != a {
            self.set_active(&mut o.inst, 0, a);
            // The volume's OnEnable or OnDisable (an enabled component).
            if let Some((d, v)) = &o.OptionalPostProcessing {
                match o.inst.objects[*d].active {
                    true => self.post_volume_on_enable(v.clone()),
                    false => self.post_volume_on_disable(v.id),
                }
            }
            // AudioSourceTimeScaleBehaviour.Start on its first activation (Global.AddAudioSource of the active children).
            if a {
                let (now, later): (Vec<_>, Vec<_>) = o.time_scale_starts.drain(..).partition(|&(x, ..)| o.inst.objects[x].active);
                o.time_scale_starts = later;
                for (_, transform, ambience) in now {
                    self.audio_time_scale_below(transform, ambience);
                }
            }
        }
    }
}
