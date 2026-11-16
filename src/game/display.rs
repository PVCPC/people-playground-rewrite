//! DisplayBehaviour (the Holographic Display, the Text Display and the Debug Target Damage Meter). Awake keeps the text's
//! and ColouredRenderers' colours (1.28.41; before, it drew Random.value for randomStartStrings). Use flips Activated and
//! OnDisable clears it; UpdateActivation shows or hides the text object (the light is rendering) and UpdateDisplay writes the
//! text, restores the colours and turns the rainbow on for a Value holding "UFO", which OnWillRenderObject then cycles.
//! Start adds the Edit text button (menu.rs) and runs both. The text object holds only TextMeshPro and marker scripts.
//! Edit text sets Value from its entry box (menu.rs) through `display_set_text` (the DontMindMe events are gone).
#![allow(non_snake_case)]

use super::spawn::Instance;
use super::visual::SpriteAt;
use super::{d, script, Game};

script! {
    Display {
        Activated: bool = true,
    }
    links {
        /// The component's instance id.
        id: i32 = 0,
        /// Behaviour.enabled
        enabled: bool = true,
        /// Value (a serialized string: never null).
        Value: String = String::new(),
        /// PhysicalBehaviour (Start's button goes on it).
        physical: usize = 0,
        /// TextMesh (world_text.rs).
        TextMesh: Option<usize> = None,
        /// Light (UpdateActivation switches it).
        Light: Option<SpriteAt> = None,
        /// ColouredRenderers (a serialized array: never null; a null element throws where it is read).
        ColouredRenderers: Vec<Option<SpriteAt>> = Vec::new(),
        /// Jitter: the HologramJitter's visual (visual.rs Kind::Hologram). None when null, or when the jitter has no
        /// renderer of its own (no visual), so nothing reads its Multiply.
        Jitter: Option<usize> = None,
        /// The SpriteRenderer on the display's own GameObject, whose drawing calls OnWillRenderObject (None: never called).
        own_sprite: Option<SpriteAt> = None,
        /// initialColour (TextMesh.color in Awake); None without the font data (headless: the colour is never drawn).
        initialColour: Option<[f32; 4]> = None,
        initialColours: Vec<[f32; 4]> = Vec::new(),
        rainbow: bool = false,
    }
}

/// double.ToString(): the shortest round-trip digits, in exponent form ("1.5E+20") from 1e15.
pub fn cs_double(r: f64) -> String {
    if r.is_nan() {
        "NaN".to_string()
    } else if r.is_infinite() {
        (if r < 0.0 { "-Infinity" } else { "Infinity" }).to_string()
    } else if r.abs() >= 1e15 {
        format!("{:e}", r).replace('e', "E+")
    } else {
        r.to_string()
    }
}

impl Game {
    /// DisplayBehaviour.Awake: initialColour = TextMesh.color, and initialColours = ColouredRenderers' colours (Select:
    /// a null element throws). No Random draw since 1.28.41.
    pub fn display_awake(&mut self, d: usize) {
        let text = Self::text_ref(self.displays[d].TextMesh);
        self.displays[d].initialColour = self.world_texts[text].props.as_ref().map(|p| p.font_color);
        let colours = self.displays[d].ColouredRenderers.iter().map(|r| self.sprite_color(Self::display_renderer(*r))).collect();
        self.displays[d].initialColours = colours;
    }

    fn display_renderer(r: Option<SpriteAt>) -> SpriteAt {
        r.unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"))
    }

    // DebugTargetDamageMeter.Awake: d = GetComponent<DisplayBehaviour>(), the display on its own object (Shot finds it by the physical)
    /// DebugTargetDamageMeter.Shot: `d.Value = Math.Round(shot.damage, 3).ToString()` (Math.Round(double, 3): value * 1000, round
    /// half to even, / 1000; double.ToString is the shortest round-trip text); UpdateDisplay sets the text mesh, then
    /// ModdingApi.Notify(d.Value) shows it as a notification.
    pub fn debug_target_damage_meter_shot(&mut self, p: usize, damage: f32) {
        let d = self.displays.iter().position(|d| d.physical == p).expect("the object's DisplayBehaviour (GetComponent in Awake)");
        let r = (damage as f64 * 1000.0).round_ties_even() / 1000.0;
        // double.ToString(): the shortest round-trip digits, in exponent form ("1.5E+20") from 1e15.
        self.displays[d].Value = cs_double(r);
        self.display_update_display(d);
        let v = self.displays[d].Value.clone();
        self.modding_api_notify(&v);
    }

    /// DisplayBehaviour.UpdateDisplay: TextMesh.text = Value, TextMesh.color = initialColour, Jitter.Multiply = white,
    /// each ColouredRenderer back to its initial colour, then rainbow = Value.Contains("UFO") (ordinal).
    pub fn display_update_display(&mut self, d: usize) {
        let (text, value) = (self.displays[d].TextMesh, self.displays[d].Value.clone());
        self.text_write(text, &value);
        if let Some(c) = self.displays[d].initialColour {
            self.text_color_set(Self::text_ref(text), c);
        }
        if let Some(v) = self.displays[d].Jitter {
            self.hologram_set_multiply(v, [1.0; 4]);
        }
        for i in 0..self.displays[d].ColouredRenderers.len() {
            let r = Self::display_renderer(self.displays[d].ColouredRenderers[i]);
            let c = self.displays[d].initialColours[i];
            self.set_sprite_color(r, c);
        }
        self.displays[d].rainbow = value.contains("UFO");
    }

    /// DisplayBehaviour.OnWillRenderObject of each enabled display whose own renderer the camera draws: while rainbow,
    /// Color.HSVToRGB(Time.time * 0.5 % 1, 1, 1) goes to TextMesh.color, Jitter.Multiply and each ColouredRenderer (its
    /// alpha kept). Called before the HologramJitters' (visual_will_render): the display's renderer is on the root, the
    /// jitter's on a child.
    pub fn displays_will_render(&mut self) {
        for i in 0..self.displays.len() {
            let x = &self.displays[i];
            let Some(own) = x.own_sprite else { continue };
            if !x.rainbow || !self.will_render_dispatch(x.id, x.enabled && !self.destroyed_components.contains(&x.id) && self.sprite_drawn(own)) {
                continue;
            }
            let h = ((d(self.time.time) * 0.5f32).0 % 1.0) as f32;
            let c = super::menu::hsv_to_rgb([h, 1.0, 1.0]);
            let text = Self::text_ref(self.displays[i].TextMesh);
            self.text_color_set(text, c);
            if let Some(v) = self.displays[i].Jitter {
                self.hologram_set_multiply(v, c);
            }
            for k in 0..self.displays[i].ColouredRenderers.len() {
                let r = Self::display_renderer(self.displays[i].ColouredRenderers[k]);
                let a = self.sprite_color(r)[3];
                self.set_sprite_color(r, [c[0], c[1], c[2], a]);
            }
        }
    }

    /// DisplayBehaviour.UpdateActivation: Light.enabled = Activated, TextMesh.gameObject.SetActive(Activated), then UpdateDisplay while Activated.
    pub fn display_update_activation(&mut self, d: usize, inst: &mut Instance) {
        let on = self.displays[d].Activated;
        let light = Self::display_renderer(self.displays[d].Light);
        self.set_sprite_enabled(light, on);
        let text = Self::text_ref(self.displays[d].TextMesh);
        let object = self.world_texts[text].object;
        self.set_active_scripts(inst, object, on);
        if on {
            self.display_update_display(d);
        }
    }

    /// DisplayBehaviour.Start's UpdateDisplay and UpdateActivation.
    pub fn display_start(&mut self, d: usize) {
        self.display_update_display(d);
        let id = self.displays[d].id;
        self.with_component_instance(id, |g, inst, _| g.display_update_activation(d, inst));
    }

    /// DisplayBehaviour.Use: `if (base.enabled)` Activated flips, then UpdateActivation.
    pub fn display_use(&mut self, d: usize) {
        if self.displays[d].enabled {
            self.displays[d].Activated = !self.displays[d].Activated;
            let id = self.displays[d].id;
            self.with_component_instance(id, |g, inst, _| g.display_update_activation(d, inst));
        }
    }

    /// DisplayBehaviour.OnDisable: Activated = false, then UpdateActivation.
    pub fn display_on_disable(&mut self, d: usize, inst: &mut Instance) {
        self.displays[d].Activated = false;
        self.display_update_activation(d, inst);
    }

    /// DisplayBehaviour.setTextFromUserInput (static since 1.28.41), the Edit text dialog's callback: t.Value = value, then t.UpdateDisplay.
    pub fn display_set_text(&mut self, d: usize, value: String) {
        self.displays[d].Value = value;
        self.display_update_display(d);
    }
}
