//! GeneratorBehaviour, SingleFloodlightBehaviour, SpikeGrenadeBehaviour, HeatingElementBehaviour,
//! TemperatureRayGunBehaviour and BulbBehaviour. Audio, particles, sprite and light toggles, property blocks and the context menu
//! button are presentation; the activation, charge, temperature, coroutine, spawn and raycast state stays.
#![allow(non_snake_case)]

use super::destroyable::Step;
use super::explosion::{EffectSize, ExplosionParameters};
use super::{d, mathf, script, Game};
use crate::assets::db::ObjRef;
use crate::b2::math::Vec2 as V;
use crate::unity::transform::{from_to_rotation, global_position, global_rotation, Trs};

script! {
    Generator {
        Activated: bool = false,
        TargetCharge: f32 = 100.0,
        TargetEnergyReachSpeed: f32 = 1.0,
        Vibrate: bool = false,
        VibrateForce: f32 = 0.0,
        VibrateRotationSpeed: f32 = 0.0,
        WarmupProgress: f32 = 0.0,
        InstantChargeChange: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// physicalBehaviour
        Phys: usize = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// GeneratorBehaviour.propertyBlock (Start copies the renderer's block into it).
        block: super::props::PropBlock = Default::default(),
    }
}

script! {
    Floodlight {
        Activated: bool = false,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// ToToggle: the clone's object index.
        ToToggle: Option<usize> = None,
        /// SwitchSounds and OnSwitchSound lengths (PickRandom draws from them).
        SwitchSounds: usize = 0,
        OnSwitchSound: usize = 0,
        /// PhysicalBehaviour (its Charge), Renderer and the propertyBlock Awake read from it.
        PhysicalBehaviour: usize = 0,
        Renderer: Option<super::visual::SpriteAt> = None,
        propertyBlock: super::props::PropBlock = super::props::PropBlock(Default::default()),
        /// Highlights.
        Highlights: Vec<Highlight> = Vec::new(),
    }
}

/// SingleFloodlightBehaviour.Highlight; its PropertyBlock is read from the SpriteRenderer in Initialise.
#[derive(Clone, Debug)]
pub struct Highlight {
    pub SpriteRenderer: super::visual::SpriteAt,
    pub Colour: [f32; 4],
    pub Intensity: f32,
    pub PropertyBlock: super::props::PropBlock,
}

script! {
    SpikeGrenade {
        SpikeCount: i32 = 12,
        DirectRange: f32 = 4.0,
        SpikeForce: f32 = 10.0,
        Delay: f32 = 2.0,
        Ignited: bool = false,
    }
    links {
        /// The component's instance id (its coroutine's owner).
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// GetComponent<PhysicalBehaviour>()
        Phys: usize = 0,
        SpikePrefab: Option<ObjRef> = None,
        ExplosionVFX: Option<ObjRef> = None,
        /// CollisionLayers (LayerMask bits)
        CollisionLayers: u32 = 0,
        /// The Light SpriteRenderer (None: a null field).
        Light: Option<super::visual::SpriteAt> = None,
    }
}

script! {
    HeatingElement {
        AllowedTemperatureRange: [f32; 2] = [0.0, 1000.0],
        HeatTargetSpeed: f32 = 0.1,
        ActiveCooling: bool = false,
        TargetTemperature: f32 = 1.0,
        Activated: bool = false,
        Color: [f32; 4] = [0.0; 4],
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// PhysicalBehaviour
        Phys: usize = 0,
        /// DamagableMachineryBehaviour (the `Game::machinery` index)
        Machinery: usize = 0,
        ActivationLight: Option<super::visual::SpriteAt> = None,
        Glow: Option<super::visual::SpriteAt> = None,
    }
}

script! {
    RayGun {
        TemperatureDelta: f32 = 5.0,
    }
    links {
        useHeat: f32 = 0.0,
        /// The component's instance id.
        id: i32 = 0,
        /// PhysicalBehaviour
        Phys: usize = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// Layers (LayerMask bits)
        Layers: u32 = 0,
        /// The local TRS of the LineRenderer's object and of each parent below the gun's own object.
        Barrel: Vec<Trs> = Vec::new(),
        /// LineRenderer (Game::lines)
        line: usize = 0,
    }
}

script! {
    Bulb {
        ChargeInfluence: f32 = 1.0,
        MaximumTemperature: f32 = 300.0,
        TargetFilamentTemperature: f32 = 2800.0,
        MinimumImpactForce: f32 = 0.0,
        FilamentHealth: f32 = 1.0,
        FilamentTemp: f32 = 0.0,
        Broken: bool = false,
        Activated: bool = false,
    }
    links {
        /// The component's instance id (its coroutine's owner).
        id: i32 = 0,
        /// PhysicalBehaviour
        Phys: usize = 0,
        /// FilamentColour (UpdateColor).
        FilamentColour: super::visual::Gradient = super::visual::Gradient::default(),
        /// Filament and LightSprite (Break disables them).
        Filament: Option<super::visual::SpriteAt> = None,
        LightSprite: Option<super::visual::SpriteAt> = None,
        /// BrokenSprite and its bounds on the physical's renderer (Break sets it).
        BrokenSprite: Option<(crate::assets::db::ObjRef, ([f32; 3], [f32; 3]))> = None,
        /// OtherPart: the clone's object index.
        OtherPart: Option<usize> = None,
        /// Explosion's ExplosionSoundBehviour (GetComponentInChildren): its instance id, the Near count when Play
        /// draws (enabled, with an AudioSource) and its DistantSound.
        sound: Option<(i32, Option<usize>, i64)> = None,
        /// ColliderToDisable's instance id (a collider on the bulb's own object).
        collider: Option<i32> = None,
        /// private flicker
        flicker: bool = false,
        /// The component's index on its GameObject, and whether the PhysicalBehaviour precedes it.
        index: usize = 0,
        after_physical: bool = true,
    }
}

/// Vector3.normalized: (float)Math.Sqrt(x * x + y * y + z * z) in double, then a division.
fn normalized3(v: [f32; 3]) -> [f32; 3] {
    let num = (d(v[0]) * v[0] + d(v[1]) * v[1] + d(v[2]) * v[2]).0.sqrt() as f32;
    if num > 1e-5 { [v[0] / num, v[1] / num, v[2] / num] } else { [0.0; 3] }
}

impl Game {
    // ---- GeneratorBehaviour ----

    // GeneratorBehaviour.Awake: `GetComponent<SpriteRenderer>` is the physical's sprite; the block is read when linked (link.rs).
    // GeneratorBehaviour.Start: `renderer.GetPropertyBlock(propertyBlock)` is link.rs (`block`); UpdateActivation is audio and particles.
    // BulbBehaviour.System.Collections.IEnumerator.get_Current: the WaitAFrame iterator's plumbing (BulbBehaviour.Shot).
    // BulbBehaviour.System.Collections.Generic.IEnumerator<System.Object>.get_Current: likewise.
    // BulbBehaviour.System.Collections.IEnumerator.Reset: likewise.
    // BulbBehaviour.System.IDisposable.Dispose: likewise.
    /// GeneratorBehaviour.Start: UpdateActivation.
    pub fn generator_start(&mut self, i: usize) {
        self.generator_update_activation(i);
    }

    /// GeneratorBehaviour.UpdateActivation: generatorAudioSource and particles Play or Stop.
    pub(crate) fn generator_update_activation(&mut self, i: usize) {
        let s = &self.generators[i];
        if let Some(src) = self.audio.source_field(s.id, "generatorAudioSource") {
            if s.Activated { self.audio.play(src) } else { self.audio.stop(src) }
        }
        // gapC r11: `if ((bool)particles) particles.Play()` / `.Stop()`.
        let (id, on) = (self.generators[i].id, self.generators[i].Activated);
        self.ps_field_play_stop(id, "particles", on);
    }

    /// GeneratorBehaviour.Use
    pub fn generator_use(&mut self, i: usize) {
        let s = &mut self.generators[i];
        if s.enabled {
            s.WarmupProgress = 0.0;
            s.Activated = !s.Activated;
            self.generator_update_activation(i);
        }
    }

    /// GeneratorBehaviour.OnWillRenderObject (its object's sprite drawn): `_Progress` is the charge over the target on the sprite's block.
    pub fn generators_will_render(&mut self) {
        for i in 0..self.generators.len() {
            let s = &self.generators[i];
            let at = super::visual::SpriteAt::Physical(s.Phys);
            if !self.will_render_dispatch(s.id, s.enabled && !self.disabled_ids.contains(&s.id) && self.sprite_drawn(at)) {
                continue;
            }
            let value = self.physicals[s.Phys].Charge() / s.TargetCharge;
            let mut block = std::mem::take(&mut self.generators[i].block);
            block.set_float("_Progress", value);
            self.set_property_block(at, &block);
            self.generators[i].block = block;
        }
    }

    /// GeneratorBehaviour.OnDisable
    pub fn generator_on_disable(&mut self, i: usize) {
        self.generators[i].Activated = false;
        let id = self.generators[i].id;
        if let Some(src) = self.audio.source_field(id, "generatorAudioSource") {
            self.audio.stop(src);
        }
        self.ps_field_play_stop(id, "particles", false);
    }

    /// GeneratorBehaviour.Update
    pub fn generator_update(&mut self, i: usize) {
        let s = &mut self.generators[i];
        s.WarmupProgress = mathf::clamp01(s.WarmupProgress);
        let (p, target, warmup) = (s.Phys, s.TargetCharge, s.WarmupProgress);
        if s.Activated {
            let charge = self.physicals[p].Charge();
            let new = if s.InstantChargeChange { mathf::max(charge, target) } else { mathf::max(charge, mathf::lerp(0.0, target, warmup)) };
            self.physicals[p].charge = new;
            let smooth = self.time.smoothDeltaTime;
            let s = &mut self.generators[i];
            s.WarmupProgress = (d(s.WarmupProgress) + d(smooth) * 60.0f32 * s.TargetEnergyReachSpeed).f();
        } else if s.InstantChargeChange {
            self.physicals[p].charge = 0.0;
        }
        // generatorAudioSource.volume = Charge / TargetCharge * 0.2f
        let s = &self.generators[i];
        if let Some(src) = self.audio.source_field(s.id, "generatorAudioSource") {
            self.audio.get_mut(src).set_volume(self.physicals[p].Charge() / target * 0.2);
        }
    }

    /// GeneratorBehaviour.FixedUpdate
    pub fn generator_fixed_update(&mut self, i: usize) {
        let s = &self.generators[i];
        if !s.Activated {
            return;
        }
        let (p, target) = (s.Phys, s.TargetCharge);
        let num = target / 4.0;
        if self.physicals[p].Temperature < num {
            let t = self.physicals[p].Temperature;
            self.physicals[p].Temperature = mathf::lerp(t, num, 0.02 / self.heat_capacity(p));
        }
        let s = &self.generators[i];
        if s.Vibrate {
            let v = self.generator_force_vector(i);
            let force = [v[0] * s.WarmupProgress, v[1] * s.WarmupProgress];
            let rb = self.physicals[p].rigidbody.clone();
            rb.add_relative_force(&mut self.world, force);
        }
    }

    /// GeneratorBehaviour.get_ForceVector: new Vector2(Mathf.Cos(f), Mathf.Sin(f)) * VibrateForce, f = Time.time * VibrateRotationSpeed.
    pub(crate) fn generator_force_vector(&self, i: usize) -> [f32; 2] {
        let s = &self.generators[i];
        let f = self.time.time * s.VibrateRotationSpeed;
        [((f as f64).cos() as f32) * s.VibrateForce, mathf::sin(f) * s.VibrateForce]
    }

    // ---- SingleFloodlightBehaviour ----

    /// SingleFloodlightBehaviour.Start
    pub fn floodlight_start(&mut self, i: usize) {
        self.floodlight_update_activation(i);
    }

    /// SwitchSoundSource (a null one throws).
    pub(crate) fn floodlight_source(&self, i: usize) -> i32 {
        self.audio.source_field(self.floodlights[i].id, "SwitchSoundSource").unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
    }

    /// SingleFloodlightBehaviour.Use
    pub fn floodlight_use(&mut self, i: usize) {
        if self.floodlights[i].enabled {
            self.floodlights[i].Activated = !self.floodlights[i].Activated;
            self.floodlight_update_activation(i);
            let (id, n) = (self.floodlights[i].id, self.floodlights[i].SwitchSounds);
            let clip = self.pick_clip(id, "SwitchSounds", n);
            let src = self.floodlight_source(i);
            self.audio.play_one_shot(src, clip.as_ref(), 1.0);
        }
    }

    /// SingleFloodlightBehaviour.UpdateActivation: the sounds, the draw and ToToggle.SetActive.
    pub(crate) fn floodlight_update_activation(&mut self, i: usize) {
        let s = &self.floodlights[i];
        let (active, to_toggle, id, on_switch) = (s.Activated, s.ToToggle, s.id, s.OnSwitchSound);
        let src = self.floodlight_source(i);
        if active {
            self.audio.play(src);
            if on_switch != 0 {
                let clip = self.pick_clip(id, "OnSwitchSound", on_switch);
                self.audio.play_one_shot(src, clip.as_ref(), 1.0);
            }
        } else {
            self.audio.stop(src);
        }
        let to_toggle = to_toggle.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.with_component_instance(id, |g, inst, _| g.set_active_scripts(inst, to_toggle, active));
    }

    /// SingleFloodlightBehaviour.Awake: propertyBlock (when there is a Renderer) and each Highlight's PropertyBlock are read from their
    /// SpriteRenderers (Highlight.Initialise).
    pub fn floodlight_awake(&mut self, i: usize) {
        if let Some(r) = self.floodlights[i].Renderer {
            self.floodlights[i].propertyBlock = self.get_property_block(r);
        }
        for h in 0..self.floodlights[i].Highlights.len() {
            self.floodlights[i].Highlights[h].PropertyBlock = self.get_property_block(self.floodlights[i].Highlights[h].SpriteRenderer);
        }
    }

    /// SingleFloodlightBehaviour.Update: `_GlowIntensity` on the Renderer's block (Charge / 20 * 0.01 + 0.09 while Activated, else 0) and
    /// on each highlight's (Charge / 20 + Intensity), which is set on its sprite at once, and the highlight's colour lerped to white by it.
    pub fn floodlight_update(&mut self, i: usize) {
        let s = &self.floodlights[i];
        let num = self.physicals[s.PhysicalBehaviour].Charge() / 20.0;
        let on = s.Activated;
        if s.Renderer.is_some() {
            self.floodlights[i].propertyBlock.set_float("_GlowIntensity", if on { num * 0.01 + 0.09 } else { 0.0 });
        }
        for h in 0..self.floodlights[i].Highlights.len() {
            let mut hl = std::mem::replace(&mut self.floodlights[i].Highlights[h].PropertyBlock, super::props::PropBlock(Default::default()));
            let (at, colour, intensity) = {
                let x = &self.floodlights[i].Highlights[h];
                (x.SpriteRenderer, x.Colour, x.Intensity)
            };
            hl.set_float("_GlowIntensity", if on { num + intensity } else { 0.0 });
            self.set_property_block(at, &hl);
            self.floodlights[i].Highlights[h].PropertyBlock = hl;
            self.set_sprite_color(at, super::visual::color_lerp(colour, [1.0; 4], num));
        }
    }

    /// SingleFloodlightBehaviour.OnWillRenderObject for each floodlight whose sprite the camera draws: Renderer.SetPropertyBlock(propertyBlock).
    pub fn floodlights_will_render(&mut self) {
        for i in 0..self.floodlights.len() {
            let s = &self.floodlights[i];
            let Some(r) = s.Renderer else { continue };
            if !self.will_render_dispatch(s.id, s.enabled && !self.disabled_ids.contains(&s.id) && self.sprite_drawn(r)) {
                continue;
            }
            let block = std::mem::take(&mut self.floodlights[i].propertyBlock);
            self.set_property_block(r, &block);
            self.floodlights[i].propertyBlock = block;
        }
    }

    /// SingleFloodlightBehaviour.OnDisable on the clone `inst`: Activated = false, SwitchSoundSource.Stop(), ToToggle.SetActive(false),
    /// and each highlight's `_GlowIntensity` 0 on its sprite.
    pub fn floodlight_on_disable(&mut self, i: usize, inst: &mut super::spawn::Instance) {
        self.floodlights[i].Activated = false;
        for h in 0..self.floodlights[i].Highlights.len() {
            let mut hl = std::mem::replace(&mut self.floodlights[i].Highlights[h].PropertyBlock, super::props::PropBlock(Default::default()));
            hl.set_float("_GlowIntensity", 0.0);
            self.set_property_block(self.floodlights[i].Highlights[h].SpriteRenderer, &hl);
            self.floodlights[i].Highlights[h].PropertyBlock = hl;
        }
        let src = self.floodlight_source(i);
        self.audio.stop(src);
        let to_toggle = self.floodlights[i].ToToggle.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.set_active_scripts(inst, to_toggle, false);
    }

    // ---- SpikeGrenadeBehaviour ----

    /// SpikeGrenadeBehaviour.Start: Light.enabled = false, then the routine when already ignited.
    pub fn spike_grenade_start(&mut self, i: usize) {
        let light = self.spike_grenades[i].Light.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.set_sprite_enabled(light, false);
        if self.spike_grenades[i].Ignited {
            self.spike_grenade_start_routine(i);
        }
    }

    /// SpikeGrenadeBehaviour.Use
    pub fn spike_grenade_use(&mut self, i: usize) {
        if self.spike_grenades[i].enabled {
            self.spike_grenade_ignite(i);
        }
    }

    /// SpikeGrenadeBehaviour.Ignite
    pub fn spike_grenade_ignite(&mut self, i: usize) {
        if !self.spike_grenades[i].Ignited {
            self.spike_grenades[i].Ignited = true;
            self.spike_grenade_start_routine(i);
        }
    }

    /// StartCoroutine(IgniteRoutine()) up to its WaitForSeconds(Delay).
    fn spike_grenade_start_routine(&mut self, i: usize) {
        let delay = self.spike_grenade_routine_first(i);
        self.start_wait(self.spike_grenades[i].id, delay, Step::SpikeIgnite(i));
    }

    /// IgniteRoutine's first MoveNext: Light.enabled = true, PlayClipOnce(IgniteSound, 2f); it yields WaitForSeconds(Delay).
    pub(crate) fn spike_grenade_routine_first(&mut self, i: usize) -> f32 {
        let light = self.spike_grenades[i].Light.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.set_sprite_enabled(light, true);
        let s = &self.spike_grenades[i];
        let (id, delay, p) = (s.id, s.Delay, s.Phys);
        let clip = self.audio.clip(id, "IgniteSound");
        self.play_clip_once(p, clip, 2.0);
        delay
    }

    /// SpikeGrenadeBehaviour.OnDisable: StopCoroutine("IgniteRoutine") does not stop a coroutine started from an
    /// IEnumerator (SetActive(false) does, in `set_active_scripts`); the rest is the Light.
    pub fn spike_grenade_on_disable(&mut self, i: usize) {
        self.spike_grenades[i].Ignited = false;
        self.main_audio_stop(self.spike_grenades[i].Phys);
        if let Some(light) = self.spike_grenades[i].Light {
            self.set_sprite_enabled(light, false);
        }
    }

    /// SpikeGrenadeBehaviour.IgniteRoutine after its WaitForSeconds.
    pub fn spike_grenade_resume(&mut self, i: usize) {
        if !self.spike_grenades[i].Ignited {
            return;
        }
        let s = self.spike_grenades[i].clone();
        let position = self.physicals[s.Phys].transform.position;
        let origin = V::new(position[0], position[1]);
        let spike = s.SpikePrefab.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        for _ in 0..s.SpikeCount {
            let vector = mathf::normalized2(self.random.inside_unit_circle());
            // Physics2D.Raycast: the result is unused, its transform sync is not.
            self.raycast(origin, vector, s.DirectRange, s.CollisionLayers, f32::NEG_INFINITY, f32::INFINITY);
            let mut db = self.assets.take().expect("SpikeGrenadeBehaviour without the game's data");
            let spawned = db.prefab(spike).and_then(|prefab| {
                let (inst, linked) = self.spawn(&mut db, &prefab, position, [0.0, 0.0, 0.0, 1.0], None)?;
                Ok((prefab, inst, linked))
            });
            self.assets = Some(db);
            let (prefab, mut inst, linked) = spawned.unwrap_or_else(|e| panic!("SpikeGrenadeBehaviour: Instantiate: {e}"));
            // GetOrAddComponent<AudioSourceTimeScaleBehaviour>() finds the prefab's own, or adds one (an instance id; its
            // Start registers the AudioSources below the root).
            let q = *linked.physicals.get(&0).unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
            if !prefab.objects[0].components.iter().any(|(_, c)| matches!(c, crate::assets::db::Component::Script { class, .. } if class == "AudioSourceTimeScaleBehaviour")) {
                self.allocate_instance_id();
                self.physicals[q].audio_time_scale = true;
                self.pending_starts.push(super::misc::PendingStart::AudioTimeScale(inst.objects[0].components[0], false));
            }
            // obj.transform.right = vector: rotation = Quaternion.FromToRotation(Vector3.right, vector).
            inst.set_world_rotation(0, from_to_rotation([1.0, 0.0, 0.0], [vector[0], vector[1], 0.0]));
            self.refresh_transforms(&inst, &linked);
            let k = self.random.range(0.5, 1.3) * s.SpikeForce;
            let rb = self.physicals[q].rigidbody.clone();
            rb.add_force_impulse(&mut self.world, [vector[0] * k, vector[1] * k]);
            self.physicals[q].Temperature = self.physicals[s.Phys].Temperature;
            self.unsynced.push(inst);
        }
        // ExplosionCreator.CreateExplosionWithWater(IsPointUnderWater(position), new ExplosionParameters(1u, position, 0f, 0f, createFx: false))
        let under_water = self.is_point_under_water([position[0], position[1]]);
        let ex = ExplosionParameters {
            FragmentationRayCount: 1,
            Position: position,
            Range: 0.0,
            FragmentForce: 0.0,
            CreateParticlesAndSound: false,
            EffectSize: EffectSize::Medium,
            DismemberChance: 0.0,
            BallisticShrapnelCount: 0,
        };
        self.create_explosion_with_water(under_water, ex);
        let vfx = s.ExplosionVFX.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        self.spawn_prefab(vfx, position);
        self.destroy_object(s.id);
    }

    // ---- HeatingElementBehaviour ----

    /// HeatingElementBehaviour.Use
    pub fn heating_element_use(&mut self, i: usize) {
        if !self.machinery[self.heating_elements[i].Machinery].Destroyed {
            let s = &mut self.heating_elements[i];
            s.Activated = !s.Activated;
            self.heating_element_light(i);
        }
    }

    /// `ActivationLight.enabled = Activated` (HeatingElementBehaviour.Start, after its button, and Use).
    pub fn heating_element_light(&mut self, i: usize) {
        let s = &self.heating_elements[i];
        let (at, on) = (s.ActivationLight.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object")), s.Activated);
        self.set_sprite_enabled(at, on);
    }

    /// HeatingElementBehaviour.OnWillRenderObject (its object's sprite drawn): Glow fades in with the temperature.
    pub fn heating_elements_will_render(&mut self) {
        for i in 0..self.heating_elements.len() {
            let s = &self.heating_elements[i];
            if !self.will_render_dispatch(s.id, self.sprite_drawn(super::visual::SpriteAt::Physical(s.Phys)) && !self.destroyed_components.contains(&self.physicals[s.Phys].id)) {
                continue;
            }
            self.heating_element_will_render(i);
        }
    }

    /// HeatingElementBehaviour.OnWillRenderObject of element i.
    pub fn heating_element_will_render(&mut self, i: usize) {
        let s = &self.heating_elements[i];
        let [lo, hi] = s.AllowedTemperatureRange;
        let value = self.physicals[s.Phys].Temperature.clamp(lo, hi);
        let mut num = mathf::clamp01(mathf::map_range(lo, hi, 0.0, 1.0, value));
        if s.ActiveCooling {
            num = 1.0 - num;
        }
        let c = super::visual::color_lerp([0.0; 4], s.Color, num * num);
        let glow = s.Glow.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        self.set_sprite_color(glow, c);
    }

    /// HeatingElementBehaviour.FixedUpdate
    pub fn heating_element_fixed_update(&mut self, i: usize) {
        let s = &self.heating_elements[i];
        let p = s.Phys;
        let flag = s.ActiveCooling ^ (self.physicals[p].Temperature <= s.TargetTemperature);
        if s.Activated && !self.machinery[s.Machinery].Destroyed && flag {
            let (target, speed) = (s.TargetTemperature, s.HeatTargetSpeed);
            let t = self.physicals[p].Temperature;
            self.physicals[p].Temperature = mathf::lerp(t, target, speed);
        }
    }

    // ---- TemperatureRayGunBehaviour ----

    /// TemperatureRayGunBehaviour.AudioSource (a null one throws).
    fn ray_gun_source(&self, i: usize) -> i32 {
        self.audio.source_field(self.ray_guns[i].id, "AudioSource").unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
    }

    /// TemperatureRayGunBehaviour.UseContinuous: ContinuousActivationBehaviour.AssertState() only checks.
    /// (`continuous.rs` broadcasts it.)
    pub fn ray_gun_use_continuous(&mut self, i: usize) {
        if self.ray_guns[i].enabled {
            let src = self.ray_gun_source(i);
            if self.ray_guns[i].useHeat < 0.0 || !self.audio_is_playing(src) {
                let clip = self.audio.clip(self.ray_guns[i].id, "InitialiseClip");
                self.play_clip_once(self.ray_guns[i].Phys, clip, 1.5);
                self.audio.play(src);
            }
            self.ray_guns[i].useHeat = 0.1;
        }
    }

    /// TemperatureRayGunBehaviour.OnDisable: AudioSource.Stop, LineRenderer.enabled = false and Impact/Muzzle.Stop() and
    /// useHeat = 0.
    pub fn ray_gun_on_disable(&mut self, i: usize) {
        let src = self.ray_gun_source(i);
        self.audio.stop(src);
        let line = self.ray_guns[i].line;
        self.lines[line].enabled = false;
        let id = self.ray_guns[i].id;
        self.ps_field_call(id, "Impact", true, super::PsCall::Stop(self.time.timeAsDouble));
        self.ps_field_call(id, "Muzzle", true, super::PsCall::Stop(self.time.timeAsDouble));
        self.ray_guns[i].useHeat = 0.0;
    }

    /// TemperatureRayGunBehaviour.Update: while the LineRenderer is enabled, SetPosition(0, LineRenderer.transform.position).
    pub fn ray_gun_update(&mut self, i: usize) {
        let line = self.ray_guns[i].line;
        if self.lines[line].enabled {
            let (position, _) = self.ray_gun_barrel_pose(i);
            self.line_set_position(line, 0, position);
        }
    }

    /// TemperatureRayGunBehaviour.FixedUpdate: the ray from the LineRenderer's object heats what it hits; SetPosition(1) at the
    /// hit (or 100000 along), Impact moved there, and LineRenderer.enabled = useHeat > 0; Impact/Muzzle Play while
    /// heating, Stop otherwise, by isEmitting.
    pub fn ray_gun_fixed_update(&mut self, i: usize) {
        let s = &self.ray_guns[i];
        let p = s.Phys;
        if s.useHeat > 0.0 {
            let (delta, layers) = (s.TemperatureDelta, s.Layers);
            let at = self.physicals[p].transform.position;
            self.camera_shake(0.05, [at[0], at[1]]);
            let num = (d(delta) * mathf::max(1.0, self.physicals[p].Charge())).f();
            // LineRenderer.transform.position and .right
            let (position, rotation) = self.ray_gun_barrel_pose(i);
            let right = mathf::rotate(rotation, [1.0, 0.0, 0.0]);
            let scale = self.physicals[p].transform.localScale[0];
            let normalized = normalized3([right[0] * scale, right[1] * scale, right[2] * scale]);
            let hit = self.raycast(V::new(position[0], position[1]), [normalized[0], normalized[1]], f32::MAX, layers, f32::NEG_INFINITY, f32::INFINITY);
            if let Some(hit) = hit {
                self.camera_shake(0.05, [hit.point.x, hit.point.y]);
                // PhysicalObjectsInWorldByTransform.TryGetValue(raycastHit2D.transform)
                if let Some(v) = self.physical_of_collider(hit.collider).filter(|&v| self.physicals[v].in_world)
                    && self.physicals[v].SimulateTemperature
                {
                    self.physicals[v].Temperature += num;
                    // TryGetComponent<BulbBehaviour>: FilamentTemp += num * 4f
                    if let Some(b) = self.bulbs.iter().position(|b| b.Phys == v) {
                        self.bulbs[b].FilamentTemp += num * 4.0;
                    }
                }
            }
            let vector = match &hit {
                Some(h) => [h.point.x, h.point.y, 0.0],
                // position + normalized * 100000f: two Vector3 operators, each result a float.
                None => std::array::from_fn(|c| position[c] + normalized[c] * 100000.0),
            };
            let line = self.ray_guns[i].line;
            self.line_set_position(line, 1, vector);
            // Impact.transform.position = vector
            let id = self.ray_guns[i].id;
            if let Some(t) = self.field_component(id, "Impact") {
                self.with_component_instance(t, |_, inst, o| inst.set_world_position(o, vector));
                self.ps_field_moved(id, "Impact", vector);
            }
        }
        let (id, on) = (self.ray_guns[i].id, self.ray_guns[i].useHeat > 0.0);
        if !on {
            let src = self.ray_gun_source(i);
            if self.audio_is_playing(src) {
                self.audio.stop(src);
            }
        }
        self.ps_field_emit(id, "Impact", on);
        self.ps_field_emit(id, "Muzzle", on);
        let line = self.ray_guns[i].line;
        self.lines[line].enabled = self.ray_guns[i].useHeat > 0.0;
        let dt = self.time.deltaTime;
        self.ray_guns[i].useHeat -= dt;
    }

    // ---- BulbBehaviour ----

    /// BulbBehaviour.Start (`seed` is only stored)
    pub fn bulb_start(&mut self, i: usize) {
        self.random.range_int(0, 1000);
        if self.bulbs[i].Broken {
            self.bulb_break(i);
        }
    }

    /// BulbBehaviour.Update and BulbBehaviour.UpdateColor (the Filament and LightSprite colours are rendering)
    pub fn bulb_update(&mut self, i: usize) {
        let s = &mut self.bulbs[i];
        s.FilamentTemp = mathf::max(0.0, s.FilamentTemp);
        let (p, dt) = (s.Phys, self.time.deltaTime);
        if self.physicals[p].Temperature > self.bulbs[i].MaximumTemperature {
            self.bulb_break(i);
        }
        let s = &mut self.bulbs[i];
        let lerp_factor = mathf::lerp_factor_dt(0.1, dt);
        let temperature = self.physicals[p].Temperature;
        let b = (d(temperature) * 0.6f32 + d(s.FilamentTemp) * 0.4f32).f();
        s.FilamentTemp = mathf::lerp(s.FilamentTemp, b, lerp_factor);
        self.physicals[p].Temperature = mathf::lerp(temperature, b, lerp_factor);
        if s.Activated && !s.Broken && !s.flicker {
            let lerp_factor = mathf::lerp_factor_dt(0.1, dt);
            s.FilamentTemp = mathf::max(s.FilamentTemp, s.TargetFilamentTemperature * 0.4);
            let charge = self.physicals[p].Charge();
            let target = (d(s.TargetFilamentTemperature) * (0.5f32 + d(mathf::sqrt(charge * s.ChargeInfluence)))).f();
            s.FilamentTemp = mathf::max(s.FilamentTemp, mathf::lerp(s.FilamentTemp, target, lerp_factor));
        } else {
            let lerp_factor = mathf::lerp_factor_dt(0.4, dt);
            s.FilamentTemp = mathf::lerp(s.FilamentTemp, 0.0, lerp_factor);
        }
        s.flicker = false;
        self.bulb_update_color(i);
    }

    /// BulbBehaviour.UpdateColor: unbroken, Filament and LightSprite take FilamentColour at (FilamentTemp /
    /// TargetFilamentTemperature)², times FilamentHealth.
    pub fn bulb_update_color(&mut self, i: usize) {
        let s = &self.bulbs[i];
        if s.Broken {
            return;
        }
        let num = s.FilamentTemp / s.TargetFilamentTemperature;
        let color = super::visual::scale(s.FilamentColour.evaluate(num * num), s.FilamentHealth);
        for at in [s.Filament, s.LightSprite].into_iter().flatten() {
            self.set_sprite_color(at, color);
        }
    }

    /// BulbBehaviour.FixedUpdate: `|=` evaluates the draw whatever flicker is.
    pub fn bulb_fixed_update(&mut self, i: usize) {
        let flicker = self.random.value() <= 0.005;
        self.bulbs[i].flicker |= flicker;
    }

    /// BulbBehaviour.Use and BulbBehaviour.Toggle
    pub fn bulb_use(&mut self, i: usize) {
        let s = &mut self.bulbs[i];
        s.Activated = !s.Activated;
    }

    /// BulbBehaviour.OnFragmentHit
    pub fn bulb_on_fragment_hit(&mut self, i: usize) {
        if !self.bulbs[i].Broken {
            self.bulb_break(i);
        }
    }

    /// BulbBehaviour.Shot and the WaitAFrame coroutine's BulbBehaviour.MoveNext: StartCoroutine(WaitAFrame()), which breaks it after WaitForSeconds(0.05).
    pub fn bulb_shot(&mut self, i: usize) {
        if !self.bulbs[i].Broken {
            let id = self.bulbs[i].id;
            self.start_wait(id, 0.05, Step::BulbBreak(i));
        }
    }

    /// BulbBehaviour.OnCollisionEnter2D
    pub(crate) fn bulb_on_collision_enter(&mut self, i: usize, c: &crate::b2::contacts2d::ScriptCollision) {
        let s = &self.bulbs[i];
        if s.Broken {
            return;
        }
        let first = c.contacts.first().unwrap_or_else(|| crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array."));
        if first.normal_impulse < s.MinimumImpactForce {
            return;
        }
        if let Some(o) = self.physical_of_collider(c.collider).filter(|&o| self.physicals[o].rigidbody.bodyType == 0) {
            let rb = self.physicals[o].rigidbody.clone();
            let (v, prev) = (rb.velocity(&self.world), self.physicals[o].previousVelocity);
            rb.set_velocity(&mut self.world, [mathf::lerp(v[0], prev[0], 1.0), mathf::lerp(v[1], prev[1], 1.0)]);
        }
        self.bulb_break(i);
    }

    /// BulbBehaviour.OnDestroy: Object.Destroy(OtherPart) (a null OtherPart destroys nothing).
    pub fn bulb_on_destroy(&mut self, i: usize) {
        if let Some(other) = self.bulbs[i].OtherPart {
            let t = self.with_component_instance(self.bulbs[i].id, |_, inst, _| inst.objects[other].components[0]);
            self.destroy_object(t);
        }
    }

    /// BulbBehaviour.Break(velocity): the velocity is unused.
    pub fn bulb_break(&mut self, i: usize) {
        if !self.bulbs[i].Broken {
            // Explosion.Play(withChildren: true)
            let sid = self.bulbs[i].id;
            self.ps_field_call(sid, "Explosion", true, super::PsCall::Play);
            // Explosion.GetComponentInChildren<ExplosionSoundBehviour>().Play(): audioSource.clip = Near.PickRandom() and
            // PlayDelayed(0), then the delayed distant sound.
            let (id, near, distant) = self.bulbs[i].sound.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
            if let Some(n) = near {
                let k = self.pick_random(n);
                self.explosion_sound_play_clip(id, k, 0.0);
                if self.prefs.DistantSoundEffects && distant != 0 {
                    self.start_wait(id, 0.0, Step::DistantSound(distant));
                }
            }
            self.bulbs[i].Broken = true;
        }
        if let Some(other) = self.bulbs[i].OtherPart {
            let id = self.bulbs[i].id;
            // OtherPart.SetActive(true): its first activation wakes the scripts it carries.
            let other_id = self.with_component_instance(id, |g, inst, _| {
                g.set_active_scripts(inst, other, true);
                inst.objects[other].components[0]
            });
            // OtherPart.transform.SetParent(null)
            self.set_parent_null_of(other_id, |_, _, _| {});
            // OtherPart.GetOrAddComponent<DebrisComponent>() and this GameObject's.
            self.with_component_instance(id, |g, inst, o| {
                g.get_or_add_debris(inst, other);
                g.get_or_add_debris(inst, o);
            });
        }
        // PhysicalBehaviour.spriteRenderer.sprite = BrokenSprite (the selection outline's sprite is UI), then
        // Filament.enabled = false and LightSprite.enabled = false.
        let nre = || -> ! { crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        let (p, broken) = (self.bulbs[i].Phys, self.bulbs[i].BrokenSprite);
        let ph = &mut self.physicals[p];
        let Some((sprite, bounds)) = broken else { unimplemented!("mod-only: BulbBehaviour.BrokenSprite is null") };
        ph.sprite.as_mut().unwrap_or_else(|| nre()).0 = sprite;
        ph.sprite_bounds = Some(bounds);
        let filament = self.bulbs[i].Filament.unwrap_or_else(|| nre());
        self.set_sprite_enabled(filament, false);
        let light = self.bulbs[i].LightSprite.unwrap_or_else(|| nre());
        self.set_sprite_enabled(light, false);
        // ColliderToDisable.enabled = false (Collider2D::SetEnabled).
        let collider = self.bulbs[i].collider.unwrap_or_else(|| crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        let body = self.physicals[self.bulbs[i].Phys].rigidbody.body;
        self.collider_disable_one(collider, body);
    }

    /// GetOrAddComponent<DebrisComponent> on clone object `o`: a new one takes an instance id (it has no messages).
    fn get_or_add_debris(&mut self, inst: &super::spawn::Instance, o: usize) {
        let has = inst.prefab.objects[o].components.iter().any(|(_, c)| matches!(c, crate::assets::db::Component::Script { class, .. } if class == "DebrisComponent"));
        if !has && self.debris_added.insert(inst.objects[o].components[0]) {
            self.allocate_instance_id();
        }
    }

    /// LineRenderer.transform's position and rotation: its local TRS up to the gun's object, then the gun's chain.
    fn ray_gun_barrel_pose(&self, i: usize) -> ([f32; 3], [f32; 4]) {
        let s = &self.ray_guns[i];
        let nodes: Vec<Trs> = s.Barrel.iter().copied().chain(self.local_chain(s.Phys).iter().rev().copied()).collect();
        let (mut v, mut r) = ([nodes[0].t[0], nodes[0].t[1], nodes[0].t[2], 0.0], nodes[0].q);
        for n in &nodes[1..] {
            (v, r) = (global_position(n, v), global_rotation(n, r));
        }
        ([v[0], v[1], v[2]], r)
    }
}

#[cfg(test)]
mod force_vector_tests {
    use super::*;

    #[test]
    fn generator_force_vector_is_cos_sin_times_force() {
        let mut g = Game::default();
        g.generators.push(Default::default());
        g.generators[0].VibrateForce = 3.0;
        g.generators[0].VibrateRotationSpeed = 0.0;
        // Time.time * 0 = 0: (cos 0, sin 0) * 3.
        assert_eq!(g.generator_force_vector(0), [3.0, 0.0]);
    }
}
