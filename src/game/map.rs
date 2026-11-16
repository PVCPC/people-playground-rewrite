//! The map's scripts that share UnityEngine.Random with the human: PlayAudioAtRandom (the ambience
//! emitter) and MapLightBehaviour (the lights switching on after the map loads).
use super::coroutine::{Routine, Yield};
use super::explosion::{EffectSize, ExplosionParameters};
use super::spawn::load_values;
use super::{d, script, Behaviour, Bounds, Game};
use crate::assets::db::{Component, ObjRef, Prefab};
use crate::b2::math::Vec2 as V;
use std::collections::HashMap;
use std::sync::Arc as Rc;

script! {
    PlayAudioAtRandom {
        ChancePerSecond: f32 = 0.5,
        ChooseRandomClip: bool = false,
        PlaceInRandomPositionInCircle: bool = false,
        OnlyOnEdge: bool = false,
        PositionCenter: [f32; 2] = [0.0, 0.0],
        Radius: f32 = 5000.0,
        t: f32 = 0.0,
    }
    links {
        /// Clips.Length; None when Clips is null.
        clips: Option<usize> = None,
        /// AudioSource.isPlaying from a recording (no AudioSource there: `source` is None).
        isPlaying: bool = false,
        /// AudioSource.transform.localPosition (xy).
        sourcePosition: [f32; 2] = [0.0, 0.0],
        /// The AudioSource (`Game::audio`).
        source: Option<i32> = None,
    }
}

script! {
    WeatherLightning {
        Width: f32 = 0.0,
        Interval: f32 = 0.1,
        Chance: f32 = 0.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Thunder.Length
        thunder: usize = 0,
        /// vertices.Length: LineRenderer.positionCount.
        vertices: usize = 0,
        /// transform.position (the clone never moves).
        position: [f32; 3] = [0.0; 3],
        t: f32 = 0.0,
    }
}

/// LayerMask.GetMask("Objects", "Bounds")
const LIGHTNING_MASK: u32 = (1 << 9) | (1 << 11);

script! {
    MapLight {
        TimeIntervalInSeconds: f32 = 0.7,
        Order: f32 = 0.0,
        Activated: bool = false,
    }
    links {
        /// Switches.Length; None when Switches is null.
        switches: Option<usize> = None,
        hasAudio: bool = false,
        /// OnGroup's Transform id (SetActive on that GameObject); None when the field is null.
        on_group: Option<i32> = None,
        /// The OnGroup GameObject's m_IsActive in the prefab: SetActive(false) at Start changes nothing unless it is true.
        on_group_active: bool = false,
        /// The pending DelayedActivation's WaitForSeconds: Time.timeAsDouble it resumes at.
        wake: Option<f64> = None,
        /// The pending coroutine is DelayedDeactivation (else DelayedActivation).
        deactivating: bool = false,
        /// The component's instance id.
        id: i32 = 0,
        /// SetPropertyBlock
        SetPropertyBlock: bool = true,
        /// spriteRenderer = GetComponent<SpriteRenderer>() (Awake).
        spriteRenderer: Option<super::visual::SpriteAt> = None,
        /// propertyBlock (Awake: the renderer's own when SetPropertyBlock).
        propertyBlock: super::props::PropBlock = super::props::PropBlock::default(),
    }
}

/// The MapConfig record of a fn-probe snapshot (tools/recorder/MapConfigProbe.cs): loaded over the linked map, compared after.
#[path = "map_config_check.rs"]
pub(crate) mod check;

/// EnvironmentalSettings (the fields a MapConfig keeps as `Settings`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvironmentalSettings {
    pub Floodlights: bool,
    pub Rain: bool,
    pub Snow: bool,
    pub Fog: bool,
    pub Gravity: f32,
    pub Lightning_chance: f32,
    pub Ambient_temperature: f32,
}

impl Default for EnvironmentalSettings {
    /// EnvironmentalSettings..ctor: Floodlights on, Gravity -9.81, Ambient_temperature 18, the rest off or 0.
    fn default() -> Self {
        EnvironmentalSettings { Floodlights: true, Rain: false, Snow: false, Fog: false, Gravity: -9.81, Lightning_chance: 0.0, Ambient_temperature: 18.0 }
    }
}

impl EnvironmentalSettings {
    /// EnvironmentalSettings.CopyTo: each field into `other`.
    pub fn copy_to(&self, other: &mut EnvironmentalSettings) {
        other.Floodlights = self.Floodlights;
        other.Rain = self.Rain;
        other.Snow = self.Snow;
        other.Fog = self.Fog;
        other.Gravity = self.Gravity;
        other.Lightning_chance = self.Lightning_chance;
        other.Ambient_temperature = self.Ambient_temperature;
    }

    /// FieldInfo.GetValue of the controller's fields in declaration order (`ENVIRONMENT`'s order); a bool reads 0 or 1.
    pub fn field(&self, i: usize) -> f32 {
        match i {
            0 => self.Floodlights as u8 as f32,
            1 => self.Rain as u8 as f32,
            2 => self.Snow as u8 as f32,
            3 => self.Fog as u8 as f32,
            4 => self.Gravity,
            5 => self.Lightning_chance,
            6 => self.Ambient_temperature,
            _ => unreachable!("EnvironmentalSettings has seven fields"),
        }
    }
}

/// The `i`th field (see `EnvironmentalSettings::field`) set to `v` (a bool field is `v != 0`).
pub fn environment_value(i: usize, v: f32) -> EnvironmentValue {
    match i {
        0 => EnvironmentValue::Floodlights(v != 0.0),
        1 => EnvironmentValue::Rain(v != 0.0),
        2 => EnvironmentValue::Snow(v != 0.0),
        3 => EnvironmentValue::Fog(v != 0.0),
        4 => EnvironmentValue::Gravity(v),
        5 => EnvironmentValue::LightningChance(v),
        6 => EnvironmentValue::AmbientTemperature(v),
        _ => unreachable!("EnvironmentalSettings has seven fields"),
    }
}

/// CreateNumberInput's text for a field of `settings` (see `Game::environment_number_text`).
pub fn environment_number_text(settings: &EnvironmentalSettings, i: usize, temperature: bool, unit: i32) -> String {
    let real = settings.field(i);
    if temperature { super::menu::cs_float(round2(Game::celsius_to_unit(real, unit))) } else { super::menu::cs_float(real) }
}

/// `(float)Math.Round(value, 2)`: times 100, rounded half to even, over 100, in double.
pub(crate) fn round2(v: f32) -> f32 {
    ((v as f64 * 100.0).round_ties_even() / 100.0) as f32
}

/// One field of EnvironmentalSettings with its new value (EnvironmentSettingsController.SetValue's `member` and `value`).
#[derive(Clone, Copy, Debug)]
pub enum EnvironmentValue {
    Floodlights(bool),
    Rain(bool),
    Snow(bool),
    Fog(bool),
    Gravity(f32),
    LightningChance(f32),
    AmbientTemperature(f32),
}

/// What MapConfig.Start reads: its BoundingBoxBehaviour's box, the parent's PlayAudioAtRandom, the
/// OverrideAmbientSounds ambience (Chance, Clips.Length), its EnvironmentalSettings, and the weather prefabs
/// (Snow, Rain, Fog; LightningCreator) with the original instance ids Instantiate orders their clones by.
#[derive(Clone, Debug)]
pub struct MapConfigStart {
    pub bounds: Bounds,
    pub audio: Option<usize>,
    pub ambience: Option<(f32, usize)>,
    /// The ambience's Volume and Clips.
    pub ambience_audio: Option<(f32, Vec<Option<super::audio::ClipRef>>)>,
    pub settings: EnvironmentalSettings,
    pub weather: Vec<Option<Rc<Prefab>>>,
    pub lightning: Option<Rc<Prefab>>,
    pub original_ids: Rc<HashMap<ObjRef, i32>>,
    /// MapConfig's Transform (the map root): the weather and lightning clones' parent.
    pub transform: i32,
    /// MapConfig.RainLoop
    pub rain_loop: Option<super::audio::ClipRef>,
}

impl PartialEq for MapConfigStart {
    /// The same MapConfig's Start: the shared prefabs compare by identity.
    fn eq(&self, o: &Self) -> bool {
        let same = |a: &Option<Rc<Prefab>>, b: &Option<Rc<Prefab>>| match (a, b) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (a, b) => a.is_none() && b.is_none(),
        };
        (self.bounds, self.audio, self.ambience, self.settings) == (o.bounds, o.audio, o.ambience, o.settings)
            && self.weather.len() == o.weather.len()
            && self.weather.iter().zip(&o.weather).all(|(a, b)| same(a, b))
            && same(&self.lightning, &o.lightning)
            && Rc::ptr_eq(&self.original_ids, &o.original_ids)
    }
}

impl Game {
    /// MapLoaderBehaviour.InstantiateMap: Object.Instantiate(map.Prefab, Vector3.zero, Quaternion.identity,
    /// WORLD.transform). WORLD sits at the origin unscaled, and its PlayAudioAtRandom is `ambient_audio[0]`
    /// (`load_main`). The Starts are queued.
    pub fn load_map(&mut self, db: &mut crate::assets::db::AssetDb, prefab: &crate::assets::db::Prefab) -> Result<(super::spawn::Instance, super::link::Linked), String> {
        if self.ambient_audio.len() != 1 {
            return Err("load_map before load_main".into());
        }
        self.spawn(db, prefab, [0.0; 3], [0.0, 0.0, 0.0, 1.0], Some(0))
    }

    /// MapLightBehaviour.Start: Activated = StartEnabled (Global.Awake clears it), UpdateActivated(playSound:
    /// true) on the off branch (rendering only), then DelayedActivation(4) up to its WaitForSeconds, whose
    /// DelayedCallManager entry is the current time plus the float seconds.
    /// OnGroup.SetActive(active) of a map light: its GameObject's scripts are enabled or disabled with it.
    /// UpdateActivated's OnActivate?.Invoke() (`on`) or OnDeactivate?.Invoke().
    pub(crate) fn map_light_event(&mut self, l: usize, on: bool) {
        let id = self.map_lights[l].id;
        self.invoke_event(id, if on { "OnActivate" } else { "OnDeactivate" });
    }

    fn map_light_set_group(&mut self, l: usize, active: bool) {
        if let Some(id) = self.map_lights[l].on_group {
            self.with_component_instance(id, |g, inst, o| g.set_active_scripts(inst, o, active));
        }
    }

    /// MapLightBehaviour.Awake: spriteRenderer = GetComponent<SpriteRenderer>(), propertyBlock = new, and with SetPropertyBlock
    /// spriteRenderer.GetPropertyBlock(propertyBlock) (a null renderer throws).
    pub(crate) fn map_light_awake(&mut self, l: usize) {
        if self.map_lights[l].SetPropertyBlock {
            let r = self.map_lights[l].spriteRenderer.unwrap_or_else(|| super::menu::throw("NullReferenceException: MapLightBehaviour without a SpriteRenderer"));
            self.map_lights[l].propertyBlock = self.get_property_block(r);
        }
    }

    /// MapLightBehaviour.UpdateActivated's property block: with SetPropertyBlock, `_GlowIntensity` 1 when `on` else 0, then
    /// spriteRenderer.SetPropertyBlock(propertyBlock). (OffGroup is null on every light of the game: link.rs stops otherwise.)
    pub(crate) fn map_light_glow(&mut self, l: usize, on: bool) {
        if self.map_lights[l].SetPropertyBlock {
            self.map_lights[l].propertyBlock.set_float("_GlowIntensity", if on { 1.0 } else { 0.0 });
            // A light rebuilt from a probe dump (dump.rs) has no renderer; a linked one has one (Awake throws otherwise).
            if let Some(r) = self.map_lights[l].spriteRenderer {
                let block = self.map_lights[l].propertyBlock.clone();
                self.set_property_block(r, &block);
            }
        }
    }

    /// MapLightBehaviour.DeactivateInstantly of a light whose GameObject is active in the hierarchy: StopAllCoroutines,
    /// Activated = false, UpdateActivated(false): OnDeactivate, OnGroup.SetActive(false).
    pub fn map_light_deactivate_instantly(&mut self, l: usize) {
        self.map_lights[l].wake = None;
        self.map_lights[l].Activated = false;
        self.map_light_event(l, false);
        self.map_light_set_group(l, false);
        self.map_light_glow(l, false);
    }

    pub fn map_light_start(&mut self, l: usize) {
        let enabled = self.map_light_start_enabled;
        self.map_lights[l].Activated = enabled;
        if enabled {
            // UpdateActivated(false) takes the on branch: OnActivate, OnGroup.SetActive(true).
            self.map_light_event(l, true);
            self.map_light_set_group(l, true);
            self.map_light_glow(l, true);
        } else {
            // UpdateActivated(true) takes the off branch: OnDeactivate, then OnGroup.SetActive(false), which
            // changes nothing for a group that starts inactive.
            self.map_light_event(l, false);
            if self.map_lights[l].on_group_active {
                self.map_light_set_group(l, false);
            }
            self.map_light_glow(l, false);
        }
        self.map_light_delayed_activation(l, 4);
    }

    /// MapLightBehaviour.DelayedActivation(factor), up to its WaitForSeconds: the Random draw is in the argument.
    pub(crate) fn map_light_delayed_activation(&mut self, l: usize, factor: i32) -> f32 {
        let m = &self.map_lights[l];
        let (interval, order) = (m.TimeIntervalInSeconds, m.Order);
        // Mono evaluates the argument in double and rounds it once at the WaitForSeconds call.
        let seconds = (d(interval) * d(self.random.range(1.0, 1.1)) * (d(order) + d(factor as f32))).f();
        self.map_lights[l].deactivating = false;
        self.map_lights[l].wake = Some(self.time.timeAsDouble + seconds as f64);
        seconds
    }

    /// MapLightBehaviour.DelayedDeactivation(factor), up to its WaitForSeconds (no Random).
    pub(crate) fn map_light_delayed_deactivation(&mut self, l: usize, factor: i32) -> f32 {
        let m = &self.map_lights[l];
        let seconds = (d(m.TimeIntervalInSeconds) * (d(m.Order) + d(factor as f32))).f();
        self.map_lights[l].deactivating = true;
        self.map_lights[l].wake = Some(self.time.timeAsDouble + seconds as f64);
        seconds
    }

    /// `gameObject.activeInHierarchy` of a map light.
    fn map_light_active_in_hierarchy(&mut self, l: usize) -> bool {
        let id = self.map_lights[l].id;
        self.with_component_instance(id, |_, inst, o| {
            let mut x = o;
            loop {
                if !inst.objects[x].active {
                    break false;
                }
                match inst.objects[x].parent {
                    Some(up) => x = up,
                    None => break true,
                }
            }
        })
    }

    /// MapLightBehaviour.Activate(delayFactor): StopAllCoroutines, then DelayedActivation, while active in the hierarchy.
    pub fn map_light_activate(&mut self, l: usize, delay_factor: i32) {
        if self.map_light_active_in_hierarchy(l) {
            self.map_lights[l].wake = None;
            self.map_light_delayed_activation(l, delay_factor);
        }
    }

    /// MapLightBehaviour.Deactivate(delayFactor): StopAllCoroutines, then DelayedDeactivation, while active in the hierarchy.
    pub fn map_light_deactivate(&mut self, l: usize, delay_factor: i32) {
        if self.map_light_active_in_hierarchy(l) {
            self.map_lights[l].wake = None;
            self.map_light_delayed_deactivation(l, delay_factor);
        }
    }

    /// MapLightBehaviour.ActivateInstantly: StopAllCoroutines, Activated = true, UpdateActivated(false): OnActivate,
    /// OnGroup.SetActive(true), while active in the hierarchy.
    pub fn map_light_activate_instantly(&mut self, l: usize) {
        if self.map_light_active_in_hierarchy(l) {
            self.map_lights[l].wake = None;
            self.map_lights[l].Activated = true;
            self.map_light_event(l, true);
            self.map_light_set_group(l, true);
            self.map_light_glow(l, true);
        }
    }

    /// MapConfig.Start: Reinitialise.
    pub fn map_config_start(&mut self, m: &MapConfigStart) {
        self.map_config = Some(m.clone());
        self.map_config_reinitialise(m);
    }

    /// MapConfig.PrepareWeatherInstance(prefab) of weather slot `k` (Snow, Rain, Fog), with Reinitialise's own steps around it: the
    /// rain's clip = RainLoop, then overlayTarget.Add(instance, new OverlayState(instance)) (map_overlay.rs) and the slot's
    /// instance field. Instantiate(prefab, MapConfig.transform) (the map root sits at the origin), position (center, 20),
    /// localScale (size, 1), SetActive(false).
    pub(crate) fn map_config_prepare_weather_instance(&mut self, m: &MapConfigStart, k: usize) {
        let Some(w) = &m.weather[k] else { return };
        let inst = self.map_config_weather_clone(m, w);
        let mut overlay = self.map_overlay_add(inst, w, (k == 1).then(|| m.rain_loop.clone()));
        overlay.kind = k;
        self.map_overlays.push(Some(overlay));
        self.map_weather[k] = Some(self.map_overlays.len() - 1);
    }

    /// MapConfig.PrepareWeatherInstance(prefab) itself: Instantiate(prefab, MapConfig.transform), position (center, 20),
    /// localScale (size, 1), SetActive(false); the new clone.
    pub(crate) fn map_config_weather_clone(&mut self, m: &MapConfigStart, w: &Prefab) -> super::spawn::Instance {
        let b = m.bounds;
        // (The recorder writes a name's spaces as '_'.)
        let recorded = self.recorded_weather.remove(&format!("{}(Clone)", w.objects[0].name).replace(' ', "_"));
        let next = recorded.map(|id| std::mem::replace(&mut self.next_instance_id, id));
        let (mut inst, behaviours) = self.instantiate_native(w, [b.center[0], b.center[1], 20.0], root_rotation(w), &m.original_ids);
        if let Some(next) = next {
            self.next_instance_id = next;
        }
        weather_awakes(w, &behaviours);
        inst.set_local_scale(0, [b.extents[0] * 2.0, b.extents[1] * 2.0, 1.0]);
        // Their OnDisables (AudioSourceTimeScaleBehaviour has none, PostProcessVolume's is rendering).
        self.set_active(&mut inst, 0, false);
        self.add_runtime_clone(m.transform, &inst.objects.iter().map(|x| (x.components[0], x.parent)).collect::<Vec<_>>());
        inst
    }

    /// MapConfig.PrepareLightningPrefab: the clone at (min.x, max.y, 0) stays active, Width = size.x, Chance = Settings.Lightning_chance;
    /// `lightning` is the new one (an earlier clone keeps running with its own Chance).
    pub(crate) fn map_config_prepare_lightning_prefab(&mut self, m: &MapConfigStart) {
        let Some(w) = &m.lightning else { super::menu::throw("ArgumentException: The Object you want to instantiate is null.") };
        let b = m.bounds;
        let chance = self.map_settings.unwrap_or(m.settings).Lightning_chance;
        let (inst, behaviours) = self.instantiate_native(w, [b.center[0] - b.extents[0], b.center[1] + b.extents[1], 0.0], root_rotation(w), &m.original_ids);
        weather_awakes(w, &behaviours);
        self.add_runtime_clone(m.transform, &inst.objects.iter().map(|x| (x.components[0], x.parent)).collect::<Vec<_>>());
        // With a Chance of 0 Update returns at once; the behaviour stays registered for ApplySettings to raise it.
        for &(o, c) in &behaviours {
            if matches!(&w.objects[o].components[c].1, Component::Script { class, .. } if class == "WeatherLightningBehaviour") {
                self.weather_lightning_add(w, &inst, o, c, b.extents[0] * 2.0, chance);
                self.map_lightning = Some(self.lightnings.len() - 1);
            }
        }
    }

    /// MapConfig.Reinitialise: the weather instances and the lightning clone (added to the earlier ones: overlayTarget keeps
    /// them, and they stay the MapConfig's children), PlayAudioAtRandom's centre, radius and ambience, then ApplySettings(Settings)
    /// (Settings is the same object: SetFloodLights changes nothing). StopAllCoroutines: MapConfig runs none.
    pub(crate) fn map_config_reinitialise(&mut self, m: &MapConfigStart) {
        for k in 0..m.weather.len() {
            self.map_config_prepare_weather_instance(m, k);
        }
        if m.lightning.is_some() {
            self.map_config_prepare_lightning_prefab(m);
        }
        if let Some(a) = m.audio {
            let s = &mut self.ambient_audio[a];
            s.PositionCenter = m.bounds.center;
            s.Radius = super::mathf::max(m.bounds.extents[0], m.bounds.extents[1]) * 1.41421;
            if let Some((chance, clips)) = m.ambience {
                s.ChancePerSecond = chance;
                s.clips = Some(clips);
                // Clips = Ambience.Clips; AudioSource.volume = Ambience.Volume
                let (volume, refs) = m.ambience_audio.clone().unwrap();
                let source = s.source;
                if let Some(c) = self.ambient_audio_clips.get_mut(a) {
                    *c = refs;
                }
                if let Some(id) = source {
                    self.audio.get_mut(id).volume = volume;
                }
            }
        }
        // `if (Settings != null) ApplySettings(Settings)`: the same object, so SetFloodLights changes nothing.
        let settings = *self.map_settings.get_or_insert(m.settings);
        self.map_config_apply_settings(settings);
    }

    /// MapConfig.ApplySettings: heatEffect.weight = Sqrt(Clamp01(MapRange(23, 1000, 0, 1, AmbientTemperature))).NaNFallback(),
    /// coldEffect.weight = MapRange(-30, 10, 1, 0, AmbientTemperature).
    pub(crate) fn map_config_post_weights(&mut self) {
        use super::mathf::{clamp01, map_range, sqrt};
        let heat = sqrt(clamp01(map_range(23.0, 1000.0, 0.0, 1.0, self.AmbientTemperature)));
        self.post_weights = [if heat.is_nan() { 0.0 } else { heat }, map_range(-30.0, 10.0, 1.0, 0.0, self.AmbientTemperature)];
    }

    /// MapConfig.SetGravity: Physics2D.gravity = settings.Gravity * Vector2.up (Physics.gravity is 3D physics).
    pub(crate) fn map_config_set_gravity(&mut self, gravity: f32) {
        self.world.gravity = crate::b2::math::Vec2::new(0.0 * gravity, 1.0 * gravity);
        // Physics2D.set_gravity (180c4cdd0): a non-zero gravity wakes every dynamic body with a non-zero gravityScale
        // (the awake flag and sleepTime = 0, awake or not).
        if self.world.gravity != crate::b2::math::Vec2::ZERO {
            for &id in &self.world.body_list {
                let b = self.world.bodies[id].as_mut().unwrap();
                if b.kind == crate::b2::world::BodyType::Dynamic && b.gravity_scale != 0.0 {
                    b.flags |= crate::b2::world::BODY_AWAKE;
                    b.sleep_time = 0.0;
                }
            }
        }
    }

    /// MapConfig.ApplySettings(settings), once a MapConfig is active: SetGravity, SetFloodLights (the lights in the order of
    /// FindObjectsOfType, which is their creation order here), the lightning Chance, PhysicalBehaviour.AmbientTemperature,
    /// the heat and cold post volumes' weights, Settings = settings and the waters' UpdateTemperatureState. The overlay targets
    /// and the foliage material are rendering (MapConfig.Update fades the overlays).
    pub fn map_config_apply_settings(&mut self, settings: EnvironmentalSettings) {
        if self.map_settings.is_none() {
            return;
        }
        self.map_config_set_gravity(settings.Gravity);
        self.map_config_set_flood_lights(&settings);
        self.map_config_set_overlay_targets(&settings);
        if let Some(l) = self.map_lightning {
            self.lightnings[l].Chance = settings.Lightning_chance;
        }
        self.AmbientTemperature = settings.Ambient_temperature;
        self.map_config_post_weights();
        self.map_settings = Some(settings);
        for i in 0..self.water_list.len() {
            self.water_update_temperature_state(self.water_list[i]);
        }
    }

    /// MapConfig.SetFloodLights: nothing happens while Floodlights is unchanged (against Settings); else each map light active in
    /// the hierarchy, in FindObjectsOfType's order (their creation order here), Activate or Deactivate.
    pub(crate) fn map_config_set_flood_lights(&mut self, settings: &EnvironmentalSettings) {
        let Some(old) = self.map_settings else { return };
        if old.Floodlights == settings.Floodlights {
            return;
        }
        for l in 0..self.map_lights.len() {
            if self.map_light_active_in_hierarchy(l) {
                if settings.Floodlights {
                    self.map_light_activate(l, 4);
                } else {
                    self.map_light_deactivate(l, 4);
                }
            }
        }
    }

    /// EnvironmentSettingsController.CreateNumberInput: the input field's text, the real value or (TemperatureSetting)
    /// TemperatureSettingAttribute.ToPreference of it in the chosen unit; None without a MapConfig (its Settings).
    /// (The field's onSelect/onDeselect Global.AddUiBlocker/RemoveUiBlocker are `Ui::env_edit` holding the UI block.)
    pub fn environment_number_text(&self, i: usize, temperature: bool, unit: i32) -> Option<String> {
        Some(environment_number_text(&self.map_settings?, i, temperature, unit))
    }

    /// Utils.CelsiusToPreference for a TemperatureUnit.
    pub(crate) fn celsius_to_unit(v: f32, unit: i32) -> f32 {
        super::Prefs { TemperatureUnit: unit, ..Default::default() }.celsius_to_preference(v)
    }

    /// EnvironmentSettingsController.CreateNumberInput's onEndEdit: float.TryParse; a temperature goes ToCelsius (rounded to two
    /// digits), clamped to the Range, SetValue, and shows ToPreference of it; any other value is clamped, SetValue and shown.
    /// Returns the input field's new text (None when the text cannot be parsed or there is no MapConfig).
    pub fn environment_number_end_edit(&mut self, i: usize, text: &str, range: Option<(f32, f32)>, temperature: bool, unit: i32) -> Option<String> {
        self.map_settings?;
        let Ok(mut result) = text.trim().parse::<f32>() else {
            eprintln!("{}", crate::il!(str "EnvironmentSettingsController/<>c__DisplayClass10_0::<CreateNumberInput>b__2" 0));
            return None;
        };
        if temperature {
            // TemperatureSettingAttribute.ToCelsius: Math.Round(Utils.PreferenceToCelsius(userInput), 2)
            result = round2(super::Prefs { TemperatureUnit: unit, ..Default::default() }.preference_to_celsius(result));
            if let Some((min, max)) = range {
                result = super::mathf::clamp(result, min, max);
            }
            self.environment_settings_set_value(environment_value(i, result));
            // TemperatureSettingAttribute.ToPreference: Math.Round(Utils.CelsiusToPreference(realValue), 2)
            result = round2(Self::celsius_to_unit(result, unit));
        } else {
            if let Some((min, max)) = range {
                result = super::mathf::clamp(result, min, max);
            }
            self.environment_settings_set_value(environment_value(i, result));
        }
        Some(super::menu::cs_float(result))
    }

    /// EnvironmentSettingsController.CreateToggle: the checkbox's isOn is the field's value, and its onValueChanged is SetValue.
    pub fn environment_toggle_value(&self, i: usize) -> Option<bool> {
        Some(self.map_settings?.field(i) != 0.0)
    }

    /// EnvironmentSettingsController.SetValue: a ShallowClone of the MapConfig's Settings with the member set, then ApplySettings.
    pub fn environment_settings_set_value(&mut self, value: EnvironmentValue) {
        // CloneUtil.ShallowClone: EnvironmentalSettings is Copy, so taking it out of `map_settings` is the MemberwiseClone.
        // CloneUtil`1.ShallowClone: clone(obj), that copy.
        // CloneUtil`1..cctor: the MemberwiseClone delegate is the derived Copy.
        let Some(mut settings) = self.map_settings else { return };
        match value {
            EnvironmentValue::Floodlights(v) => settings.Floodlights = v,
            EnvironmentValue::Rain(v) => settings.Rain = v,
            EnvironmentValue::Snow(v) => settings.Snow = v,
            EnvironmentValue::Fog(v) => settings.Fog = v,
            EnvironmentValue::Gravity(v) => settings.Gravity = v,
            EnvironmentValue::LightningChance(v) => settings.Lightning_chance = v,
            EnvironmentValue::AmbientTemperature(v) => settings.Ambient_temperature = v,
        }
        self.map_config_apply_settings(settings);
    }

    /// A recording's map: MapConfig.PrepareLightningPrefab's clone (`LightningWeather(Clone)`, a runtime child of the map
    /// root) made again with its recorded ids (the game's data in `assets`) (`root`, then its components 2 apart) at its recorded position, Width and
    /// Chance as Start gave them (the map's bounds, Settings.Lightning_chance).
    pub(crate) fn recorded_map_lightning(&mut self, map: &Prefab, root: i32, at: [f32; 3], lightning_id: i32) -> Result<(), String> {
        let (o, fields) = map.objects.iter().enumerate().find_map(|(o, x)| x.components.iter().find_map(|(_, c)| match c {
            Component::Script { class, fields, .. } if class == "MapConfig" => Some((o, fields)),
            _ => None,
        })).ok_or("a lightning clone on a map without a MapConfig")?;
        let db = self.assets.as_mut().ok_or("the map lightning without the game's data")?;
        let p = fields.get("LightningCreatorPrefab").and_then(|v| v.pptr()).filter(|p| !p.is_null()).ok_or("MapConfig.LightningCreatorPrefab")?;
        let w = db.prefab(db.resolve(map.objects[o].game_object.file, p).ok_or("LightningCreatorPrefab outside the data")?)?;
        let chance = fields.get("Settings").and_then(|s| s.get("Lightning_chance")).and_then(|v| v.f32()).ok_or("Settings.Lightning_chance")?;
        let b = map.objects[o].components.iter().find_map(|(_, c)| match c {
            Component::Script { class, fields, .. } if class == "BoundingBoxBehaviour" => Some(fields),
            _ => None,
        }).ok_or("MapConfig without a BoundingBoxBehaviour")?;
        let b = super::link::map_bounds(b)?;
        let original_ids = db.resource_load_order()?;
        let next = std::mem::replace(&mut self.next_instance_id, root);
        let (inst, behaviours) = self.instantiate_native(&w, at, root_rotation(&w), &original_ids);
        weather_awakes(&w, &behaviours);
        if let Some(t) = self.map_root {
            self.add_runtime_clone(t, &inst.objects.iter().map(|x| (x.components[0], x.parent)).collect::<Vec<_>>());
        }
        for &(o, c) in &behaviours {
            if matches!(&w.objects[o].components[c].1, Component::Script { class, .. } if class == "WeatherLightningBehaviour") {
                self.weather_lightning_add(&w, &inst, o, c, b.extents[0] * 2.0, chance);
                self.map_lightning = Some(self.lightnings.len() - 1);
            }
        }
        self.next_instance_id = next;
        match self.map_lightning.map(|l| self.lightnings[l].id) {
            Some(id) if id == lightning_id => Ok(()),
            id => Err(format!("the map lightning made again as {id:?}, recorded {lightning_id}")),
        }
    }

    /// A recording's map lightning (`id`, made by recorded_map_lightning) is the MapConfig's only one: the clone the linked map's
    /// Start made with ids of its own goes.
    pub(crate) fn keep_recorded_map_lightning(&mut self, id: i32) {
        for l in 0..self.lightnings.len() {
            if self.lightnings[l].id != id && self.destroyed_components.insert(self.lightnings[l].id) {
                self.behaviours.remove(Behaviour::WeatherLightning(l));
                // Its clone's Transform (the WeatherLightningBehaviour is the root's second component).
                let t = self.lightnings[l].id + 2;
                self.runtime_clone_transforms.values_mut().for_each(|v| v.retain(|c| c[0].0 != t));
            }
        }
        self.map_lightning = self.lightnings.iter().position(|x| x.id == id);
    }

    /// WeatherLightningBehaviour.Awake (the mask, the vertex buffer and LightSprite are rendering) with Width and Chance set by
    /// MapConfig.PrepareLightningPrefab; the behaviour joins the Update list in its execution order.
    fn weather_lightning_add(&mut self, w: &Prefab, inst: &super::spawn::Instance, o: usize, c: usize, width: f32, chance: f32) {
        let Component::Script { execution_order, fields, .. } = &w.objects[o].components[c].1 else { unreachable!() };
        let mut x = WeatherLightning::default();
        load_values(x.fields(), fields).expect("WeatherLightningBehaviour fields");
        x.Width = width;
        x.Chance = chance;
        x.id = inst.objects[o].components[c];
        x.position = inst.position(o);
        x.thunder = fields.get("Thunder").and_then(|v| v.array()).map_or(0, |a| a.len());
        let file = w.objects[o].game_object.file;
        let line = fields.get("LineRenderer").and_then(|v| v.pptr()).filter(|p| !p.is_null() && p.file == 0).unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        let (lo, lc) = *w.index.get(&ObjRef { file, path_id: line.path_id }).expect("WeatherLightningBehaviour.LineRenderer: dangling reference");
        let db = self.assets.as_ref().expect("WeatherLightningBehaviour without the game's data");
        x.vertices = crate::assets::components::line_position_count(db.data(w.objects[lo].components[lc].0)).expect("LineRenderer") as usize;
        self.lightnings.push(x);
        let (id, b) = (self.lightnings[self.lightnings.len() - 1].id, Behaviour::WeatherLightning(self.lightnings.len() - 1));
        self.behaviour_of.insert(id, (*execution_order, b));
        self.behaviours.add(*execution_order, b);
    }

    /// WeatherLightningBehaviour.Update
    pub fn weather_lightning_update(&mut self, l: usize) {
        let s = &mut self.lightnings[l];
        if super::mathf::approximately(s.Chance, 0.0) {
            return;
        }
        s.t += self.time.deltaTime;
        if s.t > s.Interval {
            s.t = 0.0;
            if self.random.value() < self.lightnings[l].Chance / 100.0 {
                let id = self.lightnings[l].id;
                self.co_start(id, Self::weather_lightning_routine(l));
            }
        }
    }

    /// WeatherLightningBehaviour.FireBolt: a bolt from a random point along the top down to the first collider, an
    /// explosion and 1500 charge there, then 2 to 4 flashes. The audio, the line points (Perlin) and the flash sprite
    /// are presentation; their Random draws stay.
    pub(crate) fn weather_lightning_routine(l: usize) -> Routine {
        #[derive(Clone, Copy)]
        enum Stage {
            Begin,
            Check(i32),
            Gap(i32),
        }
        let mut stage = Stage::Begin;
        Box::new(move |game| {
            match stage {
                Stage::Begin => {
                    let s = game.lightnings[l].clone();
                    // base.transform.position + Random.value * Width * Vector3.right
                    let k = (d(game.random.value()) * s.Width).f();
                    let vector = [s.position[0] + k, s.position[1]];
                    let Some(hit) = game.raycast(V::new(vector[0], vector[1]), [0.0, -1.0], 10000.0, LIGHTNING_MASK, f32::NEG_INFINITY, f32::INFINITY) else { return Yield::Done };
                    // x = Random.value * 10000f: the Perlin seed.
                    game.random.value();
                    game.pick_random(s.thunder);
                    for _ in 0..s.vertices {
                        game.random.inside_unit_circle();
                    }
                    let point = [hit.point.x, hit.point.y];
                    let under_water = game.is_point_under_water(point);
                    game.create_explosion_with_water(under_water, ExplosionParameters {
                        FragmentationRayCount: 16,
                        Position: [point[0], point[1], 0.0],
                        Range: 35.0,
                        FragmentForce: 12.0,
                        CreateParticlesAndSound: true,
                        EffectSize: EffectSize::Large,
                        DismemberChance: 0.0,
                        BallisticShrapnelCount: 0,
                    });
                    if let Some(p) = game.hit_world_physical(&hit) {
                        game.physicals[p].charge = game.physicals[p].Charge() + 1500.0;
                    }
                    game.camera_shake_at(150.0, point, 0.1);
                    stage = Stage::Check(0);
                }
                Stage::Check(_) => {}
                Stage::Gap(j) => {
                    // The flash off, then WaitForSeconds(Random.Range(0.032f, 0.3f)); j++.
                    let wait = game.random.range(0.032, 0.3);
                    stage = Stage::Check(j + 1);
                    return Yield::WaitForSeconds(wait);
                }
            }
            let Stage::Check(j) = stage else { unreachable!() };
            if j < game.random.range_int(2, 5) {
                stage = Stage::Gap(j);
                // The flash on, then WaitForSeconds(0.032f).
                return Yield::WaitForSeconds(0.032);
            }
            Yield::Done
        })
    }

    /// PlayAudioAtRandom.Update
    pub fn play_audio_at_random_update(&mut self, a: usize) {
        let dt = self.time.deltaTime;
        let s = &mut self.ambient_audio[a];
        if s.ChooseRandomClip && s.clips.map_or(true, |n| n == 0) {
            return;
        }
        s.t += dt;
        if !(s.t > 1.0) {
            return;
        }
        s.t = 0.0;
        let chance = s.ChancePerSecond;
        let playing = match self.ambient_audio[a].source {
            Some(id) => self.audio.is_playing(id),
            None => self.ambient_audio[a].isPlaying,
        };
        if self.random.value() <= chance && !playing {
            let s = &self.ambient_audio[a];
            if s.PlaceInRandomPositionInCircle {
                let c = self.random.inside_unit_circle();
                let s = &self.ambient_audio[a];
                let c = if s.OnlyOnEdge { super::mathf::normalized2(c) } else { c };
                self.ambient_audio[a].sourcePosition = [s.PositionCenter[0] + c[0] * s.Radius, s.PositionCenter[1] + c[1] * s.Radius];
            }
            let s = &self.ambient_audio[a];
            let k = if s.ChooseRandomClip { self.pick_random(s.clips.unwrap_or(0)) } else { None };
            // AudioSource.PlayOneShot(ChooseRandomClip ? Clips.PickRandom() : AudioSource.clip)
            if let Some(id) = self.ambient_audio[a].source {
                let at = self.ambient_audio[a].sourcePosition;
                let src = self.audio.get_mut(id);
                if self.ambient_audio[a].PlaceInRandomPositionInCircle {
                    (src.chain[0].t[0], src.chain[0].t[1]) = (at[0], at[1]);
                }
                let clip = if self.ambient_audio[a].ChooseRandomClip { k.and_then(|k| self.ambient_audio_clips[a].get(k).cloned().flatten()) } else { src.clip.clone() };
                self.audio.play_one_shot(id, clip.as_ref(), 1.0);
            }
        }
    }

    /// The pending MapLightBehaviour coroutines (DelayedCallManager, between Update and LateUpdate):
    /// each resumes in the first frame whose time reaches its wake, in wake order.
    pub fn map_light_coroutines(&mut self) {
        let now = self.time.timeAsDouble;
        // A light whose GameObject was destroyed stops its coroutine with it.
        for l in 0..self.map_lights.len() {
            if self.destroyed_components.contains(&self.map_lights[l].id) {
                self.map_lights[l].wake = None;
            }
        }
        let mut due: Vec<usize> = (0..self.map_lights.len()).filter(|&l| self.map_lights[l].wake.is_some_and(|w| w <= now)).collect();
        due.sort_by(|&a, &b| self.map_lights[a].wake.partial_cmp(&self.map_lights[b].wake).unwrap());
        for l in due {
            // DelayedActivation or DelayedDeactivation after its WaitForSeconds.
            // MapLightBehaviour.DelayedActivation: Activated = true, UpdateActivated().
            // MapLightBehaviour.DelayedDeactivation: Activated = false, the switch clip, UpdateActivated().
            let m = &mut self.map_lights[l];
            m.wake = None;
            let on = !m.deactivating;
            m.Activated = on;
            // UpdateActivated(playSound: true): OnActivate, the switch clip's PickRandom (DelayedDeactivation plays it
            // before UpdateActivated, which then takes the off branch: OnDeactivate, no sound), then OnGroup.SetActive.
            if on {
                self.map_light_event(l, true);
            }
            let m = &self.map_lights[l];
            if m.hasAudio {
                let (n, id) = (m.switches, m.id);
                let clip = self.pick_clip(id, "Switches", n.unwrap_or(0));
                // A light rebuilt from a probe dump (dump.rs) has `hasAudio` but no registered AudioSource.
                if let Some(src) = self.audio.source_field(id, "AudioSource") {
                    self.audio.play_one_shot(src, clip.as_ref(), 1.0);
                }
            }
            if !on {
                self.map_light_event(l, false);
            }
            self.map_light_set_group(l, on);
            self.map_light_glow(l, on);
        }
    }
}

/// A prefab root's local rotation (Instantiate under an unrotated parent keeps it).
fn root_rotation(p: &Prefab) -> [f32; 4] {
    p.objects[0].components.iter().find_map(|(_, c)| match c {
        Component::Transform(t) => Some(t.local_rotation),
        _ => None,
    }).unwrap()
}

/// A PostProcessVolume: every one in the game is global, on layer 8 (Main Camera's volume layer), blendDistance 0.
#[derive(Clone, Debug, PartialEq)]
pub struct PostVolume {
    pub id: i32,
    /// sharedProfile's name.
    pub profile: String,
    pub priority: f32,
    pub weight: f32,
}

/// The PostProcessVolume component `k` of prefab object `d`.
pub(super) fn post_volume_of(db: &crate::assets::db::AssetDb, prefab: &Prefab, d: usize, k: usize, id: i32) -> PostVolume {
    let (r, Component::Script { fields, .. }) = &prefab.objects[d].components[k] else { unreachable!() };
    let float = |n: &str| fields.get(n).and_then(crate::assets::mono::Value::f32).unwrap_or_else(|| panic!("PostProcessVolume.{n}"));
    let profile = fields.get("sharedProfile").and_then(crate::assets::mono::Value::pptr).and_then(|p| db.resolve(r.file, p));
    let profile = profile.map(|p| crate::assets::classes::MonoBehaviour::read(db.data(p)).expect("a PostProcessProfile").name);
    let profile = profile.unwrap_or_else(|| unimplemented!("mod-only: a PostProcessVolume without a profile"));
    if !matches!(profile.as_str(), "SNOW POST PROCESSING PROFILE" | "SUBSTRUCTURE POST PROCESSING PROFILE" | "UNCHARTED POST PROCESSING PROFILE" | "Meltdown profiel" | "FOG PROFILE") {
        unimplemented!("mod-only: post profile {profile}");
    }
    PostVolume { id, profile, priority: float("priority"), weight: float("weight") }
}

impl Game {
    /// PostProcessVolume.OnEnable: PostProcessManager.Register puts it at the end of its layer's list (UpdateSettings sorts that
    /// list by priority, stably).
    pub(super) fn post_volume_on_enable(&mut self, v: PostVolume) {
        self.post_volumes.retain(|x| x.id != v.id);
        self.post_volumes.push(v);
    }

    /// PostProcessVolume.OnDisable: PostProcessManager.Unregister.
    pub(super) fn post_volume_on_disable(&mut self, id: i32) {
        self.post_volumes.retain(|x| x.id != id);
    }
}

/// The weather clones' Awakes: AudioSourceTimeScaleBehaviour has none; PostProcessVolume's and
/// WeatherLightningBehaviour's (layer mask, vertex buffer, LightSprite off) are rendering.
fn weather_awakes(p: &Prefab, behaviours: &[(usize, usize)]) {
    for &(o, c) in behaviours {
        let Component::Script { class, .. } = &p.objects[o].components[c].1 else { unreachable!() };
        if !matches!(class.as_str(), "AudioSourceTimeScaleBehaviour" | "PostProcessVolume" | "WeatherLightningBehaviour") {
            unimplemented!("mod-only: {class}.Awake in a weather prefab");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::db::AssetDb;
    use crate::assets::mono::Value;
    use std::path::Path;

    /// The Default map's scripts after their Starts against dump34map's first snapshot (made with the same
    /// config.json). The lights' wake times depend on the scene's Random state and are left out, and so is PlayAudioAtRandom.t (Update has run by the snapshot).
    /// EnvironmentSettingsController: a temperature field shows ToPreference of the Celsius value in the chosen unit; the toggles and
    /// numbers map to the settings' fields in order.
    #[test]
    fn environment_controls_text_and_fields() {
        let s = EnvironmentalSettings { Floodlights: true, Rain: false, Snow: true, Fog: false, Gravity: -9.81, Lightning_chance: 0.0, Ambient_temperature: 18.0 };
        assert_eq!(environment_number_text(&s, 6, true, 0), "18");
        assert_eq!(environment_number_text(&s, 6, true, 1), "64.4");
        assert_eq!(environment_number_text(&s, 6, true, 2), "291.15");
        assert_eq!(environment_number_text(&s, 4, false, 0), "-9.81");
        assert_eq!((0..4).map(|i| s.field(i)).collect::<Vec<_>>(), vec![1.0, 0.0, 1.0, 0.0]);
        assert!(matches!(environment_value(2, 0.0), EnvironmentValue::Snow(false)));
    }

    #[test]
    fn map_scripts_match_recording() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let dump = Path::new(env!("CARGO_MANIFEST_DIR")).join("reference-probe/game/dump34map.txt");
        let Ok(mut snaps) = super::super::dump::snapshots(&dump) else { return };
        let first = snaps.next().unwrap();
        let mut db = AssetDb::open(&game).unwrap();
        let mut map = None;
        for m in db.find_scripts("Map").unwrap() {
            let (_, _, v) = db.mono(m).unwrap().unwrap();
            let p = v.get("Prefab").and_then(Value::pptr).and_then(|p| db.resolve(m.file, p)).unwrap();
            if db.game_object(p).unwrap().name == "Default map" {
                map = Some(p);
            }
        }
        let prefab = db.prefab(map.unwrap()).unwrap();
        let mut g = Game::load_main(&mut db, &game).unwrap();
        // As scene.rs does: the map instance is queued for its transform sync (and its scripts can then find it).
        g.assets = Some(Box::new(AssetDb::open(&game).unwrap()));
        let (inst, _) = g.load_map(&mut db, &prefab).unwrap();
        g.unsynced.push(inst);
        g.run_pending_starts();
        let snap = |k: &str| first.snap.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(format!("{:08x}", g.AmbientTemperature.to_bits()), snap("ambient"));
        let b = g.BoundingBox;
        assert_eq!([b.center[0], b.center[1], b.extents[0], b.extents[1]].map(|x| format!("{:08x}", x.to_bits())).join(","), snap("bounds"));
        assert_eq!((g.world.gravity.x.to_bits(), g.world.gravity.y.to_bits()), ((-0.0f32).to_bits(), (-9.81f32).to_bits()));
        let out: Vec<String> = super::super::dump::compare(&mut g, &first).into_iter().filter(|l| (l.starts_with("MapLight") || l.starts_with("PlayAudioAtRandom")) && !l.contains(".wake:") && !l.contains(".t:")).collect();
        assert!(out.is_empty(), "{} mismatches:\n{}", out.len(), out.join("\n"));
        assert_eq!(g.map_lights.len(), first.of("MapLightBehaviour").count());
    }

    #[test]
    fn environmental_settings_default_and_copy() {
        let d = EnvironmentalSettings::default();
        assert!(d.Floodlights && !d.Rain && d.Gravity == -9.81 && d.Ambient_temperature == 18.0);
        let mut o = EnvironmentalSettings { Floodlights: false, Rain: true, Snow: true, Fog: true, Gravity: 1.0, Lightning_chance: 5.0, Ambient_temperature: 0.0 };
        d.copy_to(&mut o);
        assert_eq!(o, d);
    }
}

