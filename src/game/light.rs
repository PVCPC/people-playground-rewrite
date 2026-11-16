//! GlowtubeBehaviour, CathodeLightBehaviour and LEDBulbBehaviour: activation, charge, gas, and their sprites' colours
//! and enables.
#![allow(non_snake_case)]

// LightsEffectRenderer.Init: Shader.Find("Hidden/Custom/LightPost"): lights.rs Lit's lit.wesl.
// LightsEffectRenderer.Render: lights.rs setup: the Lit pass with the profile's Intensity, MinBrightness and MaxBrightness and LightsCamera's texture as _LightsRenderTexture (always set: LightRenderCameraBehaviour.OnPreRender).

use super::explosion::{EffectSize, ExplosionParameters};
use super::{d, mathf, script, Game};
use super::visual::{mul, scale, Gradient, SpriteAt};
use crate::assets::db::ObjRef;

/// A null SpriteRenderer reference read: NullReferenceException.
fn sprite(s: Option<SpriteAt>) -> SpriteAt {
    s.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
}

script! {
    Glowtube {
        Broken: bool = false,
        Activated: bool = false,
        GasContents: f32 = 1.0,
        TargetTemperature: f32 = 35.0,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// PhysicalBehaviour
        Phys: usize = 0,
        /// Gas (Break instantiates it)
        Gas: Option<ObjRef> = None,
        /// LightSprite and ToToggle.
        LightSprite: Option<SpriteAt> = None,
        ToToggle: Option<SpriteAt> = None,
        PowerUpperbound: f32 = 0.0,
        PowerGradient: Gradient = Gradient::default(),
        /// The private initialColour (Start).
        initialColour: [f32; 4] = [0.0; 4],
    }
}

script! {
    CathodeLight {
        GasContents: f32 = 1.0,
        Indestructible: bool = false,
        Broken: bool = false,
        Activated: bool = false,
        TargetTemperature: f32 = 35.0,
        Color: [f32; 4] = [0.0; 4],
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// PhysicalBehaviour
        Phys: usize = 0,
        /// base.transform
        own: super::script_object::ObjectXf = Default::default(),
        /// Gas (Break instantiates it)
        Gas: Option<ObjRef> = None,
        /// LightSprite and ToToggle.
        LightSprite: Option<SpriteAt> = None,
        ToToggle: Option<SpriteAt> = None,
        PowerUpperbound: f32 = 0.0,
        PowerGradient: Gradient = Gradient::default(),
    }
}

script! {
    LedBulb {
        MinBrightness: f32 = 0.2,
        MaxCharge: f32 = 5.0,
        Activated: bool = false,
        lastIntensity: f32 = f32::MAX,
        colourCached: bool = false,
        Color: [f32; 4] = [0.0; 4],
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// PhysicalBehaviour
        Phys: usize = 0,
        /// LightSprite, GlowSprite and BulbMask.
        LightSprite: Option<SpriteAt> = None,
        GlowSprite: Option<SpriteAt> = None,
        BulbMask: Option<SpriteAt> = None,
        /// Behaviour.enabled
        enabled: bool = true,
    }
}

impl Game {
    // ---- GlowtubeBehaviour ----

    /// GlowtubeBehaviour.FixedUpdate
    pub fn glowtube_fixed_update(&mut self, g: usize) {
        let s = &self.glowtubes[g];
        let p = &mut self.physicals[s.Phys];
        if s.Activated && !s.Broken && p.SimulateTemperature {
            p.Temperature = mathf::lerp(p.Temperature, s.TargetTemperature, 0.01);
        }
    }

    /// GlowtubeBehaviour.get_Colour and CathodeLightBehaviour.get_ComputedColor:
    /// colour * PowerGradient(Charge / PowerUpperbound * clamped gas) * clamped gas.
    fn tube_colour(&self, colour: [f32; 4], gradient: &Gradient, p: usize, upper: f32, gas: f32) -> [f32; 4] {
        let gas = mathf::clamp01(gas);
        let t = (d(self.physicals[p].Charge()) / upper * gas).f();
        scale(mul(colour, gradient.evaluate(t)), gas)
    }

    fn glowtube_colour(&self, g: usize) -> [f32; 4] {
        let s = &self.glowtubes[g];
        self.tube_colour(s.initialColour, &s.PowerGradient, s.Phys, s.PowerUpperbound, s.GasContents)
    }

    /// GlowtubeBehaviour.Start: initialColour = ToToggle.color, then UpdateActivation.
    pub fn glowtube_start(&mut self, g: usize) {
        self.glowtubes[g].initialColour = self.sprite_color(sprite(self.glowtubes[g].ToToggle));
        self.glowtube_update_activation(g);
    }

    /// GlowtubeBehaviour.UpdateActivation: both sprites take the colour and show while Activated.
    pub(crate) fn glowtube_update_activation(&mut self, g: usize) {
        let on = self.glowtubes[g].Activated;
        for s in [self.glowtubes[g].ToToggle, self.glowtubes[g].LightSprite] {
            let c = self.glowtube_colour(g);
            self.set_sprite_color(sprite(s), c);
            self.set_sprite_enabled(sprite(s), on);
        }
    }

    /// GlowtubeBehaviour.Update
    pub fn glowtube_update(&mut self, g: usize) {
        let dt = self.time.deltaTime;
        if self.glowtubes[g].Activated {
            for s in [self.glowtubes[g].ToToggle, self.glowtubes[g].LightSprite] {
                let c = self.glowtube_colour(g);
                self.set_sprite_color(sprite(s), c);
            }
        }
        let s = &self.glowtubes[g];
        let p = s.Phys;
        if self.physicals[p].Charge() > 800.0 && !s.Broken {
            self.glowtubes[g].GasContents += dt;
            let position = self.physicals[p].transform.position;
            let under_water = self.is_point_under_water([position[0], position[1]]);
            let ex = ExplosionParameters::new(16, position, 4.0, 5.0, true, EffectSize::Medium, 0.0, 0);
            self.create_explosion_with_water(under_water, ex);
            self.glowtube_break(g);
        }
        if self.glowtubes[g].Broken {
            let c = self.glowtubes[g].GasContents;
            self.glowtubes[g].GasContents = (d(c) - d(dt) * 0.25f32).f();
        }
    }

    /// GlowtubeBehaviour.Use
    pub fn glowtube_use(&mut self, g: usize) {
        let s = &mut self.glowtubes[g];
        s.Activated = !s.Activated;
        self.glowtube_update_activation(g);
    }

    /// GlowtubeBehaviour.Shot
    pub fn glowtube_shot(&mut self, g: usize, damage: f32) {
        if damage > 50.0 {
            self.glowtube_break(g);
        }
    }

    /// GlowtubeBehaviour.Break: the gas cloud (its colour and Play are particles).
    pub fn glowtube_break(&mut self, g: usize) {
        if self.glowtubes[g].Broken {
            return;
        }
        self.glowtubes[g].Broken = true;
        let gas = self.glowtubes[g].Gas.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        let position = self.physicals[self.glowtubes[g].Phys].transform.position;
        self.spawn_prefab(gas, position);
    }

    // ---- CathodeLightBehaviour ----

    /// CathodeLightBehaviour.Start: the context menu buttons, the sprites' colour, then UpdateActivation.
    pub fn cathode_light_start(&mut self, i: usize) {
        use super::menu::{Cond, Label, MenuAction as A, MenuButton as B};
        let menu = &mut self.physicals[self.cathode_lights[i].Phys].menu;
        // !ColorpickerDialogBehaviour.IsOpen: no dialog is open while a menu is.
        menu.push(B::new("setBulbColour", crate::il!(str "CathodeLightBehaviour::Start" 1), crate::il!(str "CathodeLightBehaviour::Start" 2), A::Entry(super::menu::Entry::CathodeColour(i))));
        menu.push(B { label: Label::CathodeLightIndestructible(i), ..B::new("machineryIndestructible", "", crate::il!(str "CathodeLightBehaviour::Start" 4), A::CathodeLightToggle(i)) });
        menu.push(B { condition: Cond::CathodeLightBroken(i), ..B::new("repairMachinery", "Repair", crate::il!(str "CathodeLightBehaviour::Start" 7), A::CathodeLightRepair(i)) });
        self.cathode_light_colour_sprites(i);
        self.cathode_light_update_activation(i);
    }

    /// `ToToggle.color = ComputedColor; LightSprite.color = ComputedColor;`
    fn cathode_light_colour_sprites(&mut self, i: usize) {
        for s in [self.cathode_lights[i].ToToggle, self.cathode_lights[i].LightSprite] {
            let x = &self.cathode_lights[i];
            let c = self.tube_colour(x.Color, &x.PowerGradient, x.Phys, x.PowerUpperbound, x.GasContents);
            self.set_sprite_color(sprite(s), c);
        }
    }

    /// CathodeLightBehaviour.UpdateActivation: both sprites show while Activated.
    pub(crate) fn cathode_light_update_activation(&mut self, i: usize) {
        let on = self.cathode_lights[i].Activated;
        self.set_sprite_enabled(sprite(self.cathode_lights[i].ToToggle), on);
        self.set_sprite_enabled(sprite(self.cathode_lights[i].LightSprite), on);
    }

    /// CathodeLightBehaviour.FixedUpdate
    pub fn cathode_light_fixed_update(&mut self, i: usize) {
        let s = &self.cathode_lights[i];
        let p = &mut self.physicals[s.Phys];
        if s.Activated && !s.Broken && p.SimulateTemperature {
            p.Temperature = mathf::lerp(p.Temperature, s.TargetTemperature, 0.01);
        }
    }

    /// CathodeLightBehaviour.Update
    pub fn cathode_light_update(&mut self, i: usize) {
        self.cathode_light_colour_sprites(i);
        let dt = self.time.deltaTime;
        let p = self.cathode_lights[i].Phys;
        if !self.cathode_lights[i].Indestructible && self.physicals[p].Charge() > 800.0 && !self.cathode_lights[i].Broken {
            self.cathode_lights[i].GasContents += dt;
            // base.transform.position
            let position = self.xf_position(&self.cathode_lights[i].own);
            let under_water = self.is_point_under_water([position[0], position[1]]);
            let ex = ExplosionParameters::new(16, position, 4.0, 5.0, true, EffectSize::Medium, 0.0, 0);
            self.create_explosion_with_water(under_water, ex);
            self.cathode_light_break(i);
        }
        if self.cathode_lights[i].Broken {
            let c = self.cathode_lights[i].GasContents;
            self.cathode_lights[i].GasContents = (d(c) - d(dt) * 0.25f32).f();
        }
    }

    /// CathodeLightBehaviour.Use: `if (base.enabled && !Broken)` PlayClipOnce(ClickClip), Activated flips, then UpdateActivation.
    pub fn cathode_light_use(&mut self, i: usize) {
        let s = &self.cathode_lights[i];
        if s.enabled && !s.Broken {
            let (id, p) = (s.id, s.Phys);
            let clip = self.audio.clip(id, "ClickClip");
            self.play_clip_once(p, clip, 1.0);
            let s = &mut self.cathode_lights[i];
            s.Activated = !s.Activated;
            self.cathode_light_update_activation(i);
        }
    }

    /// CathodeLightBehaviour.Shot
    pub fn cathode_light_shot(&mut self, i: usize, damage: f32) {
        if damage > 50.0 {
            self.cathode_light_break(i);
        }
    }

    /// CathodeLightBehaviour.Break and CathodeLightBehaviour.Slice (also OnEMPHit and OnFragmentHit): the gas cloud (its colour and Play are
    /// particles).
    pub fn cathode_light_break(&mut self, i: usize) {
        if self.cathode_lights[i].Broken || self.cathode_lights[i].Indestructible {
            return;
        }
        self.cathode_lights[i].Broken = true;
        let gas = self.cathode_lights[i].Gas.unwrap_or_else(|| crate::game::menu::throw("ArgumentException: The Object you want to instantiate is null."));
        let position = self.xf_position(&self.cathode_lights[i].own);
        self.spawn_prefab(gas, position);
    }

    /// CathodeLightBehaviour.Repair (also the "repairMachinery" button).
    pub fn cathode_light_repair(&mut self, i: usize) {
        self.cathode_lights[i].GasContents = 1.0;
        self.cathode_lights[i].Broken = false;
    }

    /// The "machineryIndestructible" button: Indestructible flips; indestructible refills and mends it.
    pub fn cathode_light_toggle_indestructible(&mut self, i: usize) {
        let s = &mut self.cathode_lights[i];
        s.Indestructible = !s.Indestructible;
        if s.Indestructible {
            s.GasContents = 1.0;
            s.Broken = false;
        }
    }

    // ---- LEDBulbBehaviour ----

    /// LEDBulbBehaviour.Use
    pub fn led_use(&mut self, i: usize) {
        let s = &mut self.leds[i];
        if s.enabled {
            s.Activated = !s.Activated;
            self.led_update_activation(i);
        }
    }

    /// LEDBulbBehaviour.UpdateBulbMaskColour: the colour at full value, alpha 0.53.
    pub fn led_update_bulb_mask_colour(&mut self, i: usize) {
        let [h, s, _] = super::menu::rgb_to_hsv(self.leds[i].Color);
        let mut c = super::menu::hsv_to_rgb([h, s, 1.0]);
        c[3] = 0.53;
        self.set_sprite_color(sprite(self.leds[i].BulbMask), c);
    }

    /// LEDBulbBehaviour.UpdateActivation: GlowSprite.enabled = LightSprite.enabled = Activated.
    pub fn led_update_activation(&mut self, i: usize) {
        let s = &mut self.leds[i];
        let (on, light, glow) = (s.Activated, s.LightSprite, s.GlowSprite);
        self.set_sprite_enabled(sprite(glow), on);
        self.set_sprite_enabled(sprite(light), on);
        self.leds[i].colourCached = false;
    }

    /// LEDBulbBehaviour.OnDisable
    pub fn led_on_disable(&mut self, i: usize) {
        self.leds[i].Activated = false;
        self.led_update_activation(i);
    }

    /// LEDBulbBehaviour.Update
    pub fn led_update(&mut self, i: usize) {
        let s = &self.leds[i];
        let num = mathf::map_range(0.0, s.MaxCharge, s.MinBrightness, 1.0, self.physicals[s.Phys].Charge());
        let s = &mut self.leds[i];
        if s.colourCached {
            if !((s.lastIntensity - num).abs() > 0.01) {
                return;
            }
            s.lastIntensity = num;
            s.colourCached = false;
        }
        let mut color = scale(self.leds[i].Color, num);
        color[3] = 1.0;
        self.set_sprite_color(sprite(self.leds[i].LightSprite), color);
        self.set_sprite_color(sprite(self.leds[i].GlowSprite), scale(color, 0.4));
        self.leds[i].colourCached = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indestructible_toggle_mends_and_refills() {
        let mut g = Game::default();
        g.cathode_lights.push(CathodeLight { Broken: true, GasContents: 0.2, ..Default::default() });
        g.cathode_light_toggle_indestructible(0);
        assert!(g.cathode_lights[0].Indestructible && !g.cathode_lights[0].Broken && g.cathode_lights[0].GasContents == 1.0);
        g.cathode_light_toggle_indestructible(0);
        assert!(!g.cathode_lights[0].Indestructible);
    }

    #[test]
    fn use_needs_a_whole_light() {
        let mut g = Game::default();
        // ToToggle and LightSprite: one physical's sprite.
        g.physicals.push(Default::default());
        let s = Some(SpriteAt::Physical(0));
        g.cathode_lights.push(CathodeLight { ToToggle: s, LightSprite: s, ..Default::default() });
        g.cathode_light_use(0);
        assert!(g.cathode_lights[0].Activated);
        g.cathode_lights[0].Broken = true;
        g.cathode_light_use(0);
        assert!(g.cathode_lights[0].Activated);
        g.cathode_light_repair(0);
        g.cathode_light_use(0);
        assert!(!g.cathode_lights[0].Activated);
    }

    #[test]
    fn indestructible_or_weak_shot_does_not_break() {
        let mut g = Game::default();
        g.cathode_lights.push(CathodeLight { Indestructible: true, ..Default::default() });
        g.cathode_light_shot(0, 100.0);
        g.cathode_light_shot(0, 10.0);
        assert!(!g.cathode_lights[0].Broken);
    }
}
