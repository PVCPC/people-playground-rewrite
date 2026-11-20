//! The UI's state and its transitions, with no Bevy in them: menu navigation, the preferences store, the control scheme.

use super::tables::*;
use crate::game::{tool::Control, Prefs};
use bevy::prelude::Resource;
use std::collections::HashMap;

// ---------------------------------------------------------------- preferences

/// Every Preferences field (the settings menu's and the hidden ones), by `SETTINGS` index.
#[derive(Resource, Clone, Debug)]
pub struct Settings {
    pub values: Vec<f32>,
    /// Preferences.Resolution (a Vector2Int?): null is automatic.
    pub resolution: Option<(i32, i32)>,
}

/// Preferences' public fields in declaration order, which is the order JsonConvert writes config.json in.
pub const FILE_ORDER: &[&str] = &[
    "CollisionQuality", "ToolSelectionMode", "StopAnimationOnDamage", "DropOnDeath", "ShakeIntensity", "ShowOutlines", "SlowMotionSpeed",
    "AmbientTemperatureTransfer", "PhysicsIterations", "GridSnap", "AngleSnap", "ShowModLoadingFreeze", "RejectShadyCode",
    "MaxModCompilationTime", "GorelessMode", "ProceduralFragments", "FragilityMultiplier", "LimbCrushing", "BrainDamage",
    "LimbOverflowExplosion", "GoreMode", "AutoHealWounds", "ChunkyShotParticles", "DismembermentLooseTissue", "CrushForceMultiplier",
    "BloomMode", "TracerBullets", "FancyEffects", "Decals", "Lighting", "MasterVolume", "SfxVolume", "UserInterfaceVolume",
    "AmbienceVolume", "AmbienceHighpassCutoff", "ClampVolume", "DistantSoundEffects", "BulletCrackWhiz", "LogDebugMessages",
    "ShowFramerate", "ZoomSensitivity", "ThermalVisionUpdateRate", "DeleteWireByContextMenu", "TemperatureUnit", "VSync", "SMAA",
    "TonemappingMode", "Brightness", "RenderScale", "FramerateLimit", "SimplifiedBackground", "WindowMode", "Resolution", "ShowTutorial",
    "ToyboxSizeOffset",
];

/// Json.NET's text for a float: the shortest text that reads back, with ".0" on a whole number and an exponent form (1E-05)
/// below 1E-4 or from 1E15.
pub fn float_text(v: f32) -> String {
    let a = v.abs();
    if a != 0.0 && !(1e-4..1e15).contains(&a) {
        let e = format!("{v:E}");
        let (m, x) = e.split_once('E').unwrap();
        let (sign, digits) = x.strip_prefix('-').map_or(("+", x), |d| ("-", d));
        return format!("{m}E{sign}{digits:0>2}");
    }
    let s = format!("{v}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

/// HasTooltipBehaviour.AppendPunctuation
pub fn append_punctuation(input: &str) -> String {
    let input = input.trim();
    if [".", "!", "\u{a1}", "?", "~", ";", ":", "\"", "'", ")", "]"].iter().any(|p| input.ends_with(p)) {
        input.to_string()
    } else {
        format!("{input}.")
    }
}

/// The tooltip a "<b>title</b>\ndescription" HasTooltipBehaviour shows (OnPointerEnter: TooltipText.text = AppendPunctuation(Text)),
/// as "title\ndescription" for the two lines of the tooltip box.
pub fn tooltip(title: &str, description: &str) -> String {
    let bold = format!("<b>{title}</b>");
    let text = append_punctuation(&format!("{bold}\n{description}"));
    match text[bold.len()..].strip_prefix('\n') {
        Some(rest) => format!("{title}\n{rest}"),
        None => format!("{title}{}", &text[bold.len()..]),
    }
}

/// Utils.Snap
fn snap(v: f32, step: f32) -> f32 {
    if step.abs() < 1e-6 { v } else { (v / step).round() * step }
}

/// Preferences..ctor: every field at its default (`SETTINGS`).
impl Default for Settings {
    /// Preferences..ctor and SettingAttribute..ctor: every field at its default, each field's attribute (`SETTINGS`).
    fn default() -> Self {
        Self { values: SETTINGS.iter().map(|s| s.default).collect(), resolution: None }
    }
}

impl Settings {
    /// PreferenceExtensions.GetField: the Preferences field of that name (a name that is none throws in the game).
    pub fn index(field: &str) -> usize {
        SETTINGS.iter().position(|s| s.field == field).expect("a Preferences field")
    }

    /// PreferenceExtensions.GetByName and PreferenceExtensions.GetFloat: the field's value.
    pub fn get(&self, field: &str) -> f32 {
        self.values[Self::index(field)]
    }

    pub fn flag(&self, field: &str) -> bool {
        self.get(field) != 0.0
    }

    /// PreferenceExtensions.SetByName as the settings menu's GenerateNumberSetting and CreateRadioButton listeners call it (GetField by
    /// the Settings index, its Kind standing for the type check): the value whole for ints, snapped to the step, clamped, the nearest option
    /// for option lists.
    pub fn set(&mut self, i: usize, v: f32) {
        self.values[i] = match SETTINGS[i].kind {
            Kind::Bool => (v != 0.0) as i32 as f32,
            Kind::Enum(names) => v.round().clamp(0.0, names.len() as f32 - 1.0),
            Kind::Int { min, max, step } => snap(v.round(), step).round().clamp(min, max),
            Kind::Float { min, max, step } => snap(v, step).clamp(min, max),
            Kind::Options(options) => *options.iter().min_by(|a, b| (*a - v).abs().total_cmp(&(*b - v).abs())).unwrap(),
        };
    }

    /// 0..1 along a slider's track.
    pub fn fraction(&self, i: usize) -> f32 {
        match SETTINGS[i].kind {
            Kind::Int { min, max, .. } | Kind::Float { min, max, .. } => ((self.values[i] - min) / (max - min)).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }

    pub fn set_fraction(&mut self, i: usize, f: f32) {
        if let Kind::Int { min, max, .. } | Kind::Float { min, max, .. } = SETTINGS[i].kind {
            self.set(i, min + f.clamp(0.0, 1.0) * (max - min));
        }
    }

    /// The value as the settings menu prints it (the Format attribute, else ToString).
    pub fn text(&self, i: usize) -> String {
        let (s, v) = (&SETTINGS[i], self.values[i]);
        match s.kind {
            Kind::Bool => if v != 0.0 { "Enabled" } else { "Disabled" }.into(),
            Kind::Enum(names) => names[v as usize].into(),
            Kind::Int { .. } | Kind::Float { .. } | Kind::Options(_) => {
                let int = matches!(s.kind, Kind::Int { .. });
                match s.fmt {
                    None => if int { format!("{}", v as i32) } else { format!("{v}") },
                    Some(f) => if int { f.format_string_int(v as i32) } else { f.format_string(v) },
                }
            }
        }
    }

    /// Members of a preferences object (config.json, or our own file) over the current values; other members and
    /// values of the wrong type are ignored.
    pub fn load(&mut self, text: &str) {
        let Ok(serde_json::Value::Object(obj)) = serde_json::from_str(text) else { return };
        for (i, s) in SETTINGS.iter().enumerate() {
            let Some(v) = obj.get(s.field) else { continue };
            let v = match (v, s.kind) {
                (serde_json::Value::Bool(b), Kind::Bool) => *b as i32 as f32,
                (serde_json::Value::String(n), Kind::Enum(names)) => match names.iter().position(|x| x.eq_ignore_ascii_case(n)) {
                    Some(p) => p as f32,
                    None => continue,
                },
                (serde_json::Value::Number(n), Kind::Enum(names)) => match n.as_i64() {
                    Some(p) if (0..names.len() as i64).contains(&p) => p as f32,
                    _ => continue,
                },
                (serde_json::Value::Number(n), Kind::Int { .. }) => n.as_f64().unwrap_or(s.default as f64) as i32 as f32,
                (serde_json::Value::Number(n), Kind::Float { .. } | Kind::Options(_)) => n.as_f64().unwrap_or(s.default as f64) as f32,
                _ => continue,
            };
            // Json.NET sets the field as read: nothing snaps or clamps it.
            self.values[i] = v;
        }
        match obj.get("Resolution") {
            Some(serde_json::Value::Null) => self.resolution = None,
            Some(r) => {
                if let (Some(x), Some(y)) = (r.get("x").and_then(|v| v.as_i64()), r.get("y").and_then(|v| v.as_i64())) {
                    self.resolution = Some((x as i32, y as i32));
                }
            }
            None => {}
        }
    }

    /// UserPreferenceManager.Save's text: `JsonConvert.SerializeObject(Current, Formatting.Indented)`. The public fields in
    /// declaration order, two-space indents and Environment.NewLine, the enums by name (the Unity converters), a float with its
    /// ".0", the Resolution as {x, y} or null.
    pub fn json(&self) -> String {
        let mut lines = Vec::new();
        for field in FILE_ORDER {
            if *field == "Resolution" {
                lines.push(match self.resolution {
                    Some((x, y)) => format!("  \"Resolution\": {{\r\n    \"x\": {x},\r\n    \"y\": {y}\r\n  }}"),
                    None => "  \"Resolution\": null".into(),
                });
                continue;
            }
            let i = Self::index(field);
            let v = self.values[i];
            let text = match SETTINGS[i].kind {
                Kind::Bool => (v != 0.0).to_string(),
                Kind::Enum(names) => format!("\"{}\"", names[v as usize]),
                Kind::Int { .. } => format!("{}", v as i32),
                Kind::Float { .. } | Kind::Options(_) => float_text(v),
            };
            lines.push(format!("  \"{field}\": {text}"));
        }
        // Rewrite extras' settings (not the game's): only when changed, so an untouched file stays the game's bytes.
        for field in ["PortalSize", "PortalGlow", "PortalMishapSuperposition", "PortalMishapFold", "PortalMishapPhase"] {
            let i = Self::index(field);
            if self.values[i] != SETTINGS[i].default {
                let text = if matches!(SETTINGS[i].kind, Kind::Bool) { (self.values[i] != 0.0).to_string() } else { float_text(self.values[i]) };
                lines.push(format!("  \"{field}\": {text}"));
            }
        }
        format!("{{\r\n{}\r\n}}", lines.join(",\r\n"))
    }

    /// UserPreferenceManager.Save: a windowed game records the window's size as the Resolution, then the text above.
    pub fn save_text(&mut self, screen: (i32, i32)) -> String {
        if self.get("WindowMode") == 0.0 {
            self.resolution = Some(screen);
        }
        self.json()
    }

    /// The simulation's Preferences.
    pub fn prefs(&self) -> Prefs {
        let mut p = Prefs::default();
        let (b, f) = (|n| self.flag(n), |n| self.get(n));
        p.CollisionQuality = f("CollisionQuality") as i32;
        p.AmbientTemperatureTransfer = b("AmbientTemperatureTransfer");
        p.BrainDamage = b("BrainDamage");
        p.AutoHealWounds = b("AutoHealWounds");
        p.StopAnimationOnDamage = b("StopAnimationOnDamage");
        p.FragilityMultiplier = f("FragilityMultiplier");
        p.LimbCrushing = b("LimbCrushing");
        p.PortalSize = f("PortalSize");
        p.PortalGlow = f("PortalGlow");
        (p.PortalMishapSuperposition, p.PortalMishapFold, p.PortalMishapPhase) = (b("PortalMishapSuperposition"), b("PortalMishapFold"), b("PortalMishapPhase"));
        p.GorelessMode = b("GorelessMode");
        p.GoreMode = self.values[Self::index("GoreMode")] as i32;
        p.DropOnDeath = b("DropOnDeath");
        p.DismembermentLooseTissue = b("DismembermentLooseTissue");
        p.CrushForceMultiplier = f("CrushForceMultiplier");
        p.ProceduralFragments = b("ProceduralFragments");
        p.FancyEffects = b("FancyEffects");
        p.Decals = b("Decals");
        p.ShowOutlines = b("ShowOutlines");
        p.ZoomSensitivity = f("ZoomSensitivity");
        p.DeleteWireByContextMenu = b("DeleteWireByContextMenu");
        p.LogDebugMessages = b("LogDebugMessages");
        p.ToolSelectionMode = f("ToolSelectionMode") as i32;
        p.GridSnap = f("GridSnap");
        p.SlowMotionSpeed = f("SlowMotionSpeed");
        p.AngleSnap = f("AngleSnap") as i32;
        p.PhysicsIterations = f("PhysicsIterations") as i32;
        p.ShowTutorial = b("ShowTutorial");
        p.DistantSoundEffects = b("DistantSoundEffects");
        p.ClampVolume = b("ClampVolume");
        p.TracerBullets = b("TracerBullets");
        p.BulletCrackWhiz = b("BulletCrackWhiz");
        p.ChunkyShotParticles = b("ChunkyShotParticles");
        p.ShakeIntensity = f("ShakeIntensity");
        p.TemperatureUnit = f("TemperatureUnit") as i32;
        p.ThermalVisionUpdateRate = f("ThermalVisionUpdateRate") as i32;
        p
    }
}

/// SortAttribute..ctor(order) and OptionAttribute..ctor(params values): their Order and values are baked into `SETTINGS` (its rows are in
/// the OrderBy(Sort) order, an Option field is `Kind::Options`). The settings table's rows for a category tab, in the menu's order.
pub fn tab_rows(category: usize) -> impl Iterator<Item = usize> {
    SETTINGS.iter().enumerate().filter(move |(_, s)| s.menu && s.category == category).map(|(i, _)| i)
}

// ---------------------------------------------------------------- control scheme

/// Each action's Key and SecondaryKey (Unity names), by `ACTIONS` index.
#[derive(Resource, Clone, Debug)]
pub struct Controls {
    pub binds: Vec<(String, String)>,
}

impl Default for Controls {
    /// InputSystem.ResetToDefaults: the actions are the defaults (no mods add any).
    fn default() -> Self {
        Self { binds: ACTIONS.iter().map(|a| (a.key.to_string(), a.secondary.to_string())).collect() }
    }
}

/// Utils.UniverseMatches (InputSystem.IsCurrentUniverse and TriggerEditorBehaviour's conflict check call it).
pub(crate) fn universes_match(a: &str, b: &str) -> bool {
    a != "None" && b != "None" && (a == "All" || b == "All" || a == b)
}

impl Controls {
    pub fn index(id: &str) -> usize {
        ACTIONS.iter().position(|a| a.id == id).expect("an action")
    }

    pub fn bind(&self, id: &str) -> &(String, String) {
        &self.binds[Self::index(id)]
    }

    /// TriggerEditorBehaviour.SetText, TriggerEditorBehaviour.Start and TriggerEditorBehaviour.Awake (which only finds the Button):
    /// the KeyCode names as they are (SecondaryKey.ToString() + " + " unless None, then Key); the Trigger text's UpperCase style is the view's.
    pub fn text(&self, i: usize) -> String {
        let (k, s) = &self.binds[i];
        match s.as_str() {
            "None" => k.clone(),
            s => format!("{s} + {k}"),
        }
    }

    /// TriggerEditorBehaviour.AreConflicting and TriggerEditorBehaviour.Update (which only tints the Image) against any other action.
    pub fn conflicts(&self, i: usize) -> bool {
        let (a, b) = (&self.binds[i], ACTIONS[i]);
        (0..ACTIONS.len()).any(|j| {
            if j == i || !universes_match(b.universe, ACTIONS[j].universe) {
                return false;
            }
            let o = &self.binds[j];
            let (k, s) = (a.0 != "None", a.1 != "None");
            (k && a.0 == o.0) || (s && a.1 == o.1) || (s && a.1 == o.0 && o.0 != "None") || (o.1 != "None" && o.1 == a.0 && k)
        })
    }

    /// TriggerEditorBehaviour.SaveStateChanges: the key released and the key held with it become the action.
    pub fn set(&mut self, i: usize, key: &str, secondary: &str) {
        self.binds[i] = (key.into(), secondary.into());
    }

    /// ControlSchemeEditorBehaviour.SaveInputSystem and InputSystem.Save: `JsonConvert.SerializeObject(Actions, Formatting.Indented)`: the Dictionary in insertion order, each
    /// InputAction's public fields in declaration order, the enums by name (as the game's own ControlScheme.json has them),
    /// two-space indents and Environment.NewLine.
    pub fn json(&self) -> String {
        let text = |v: &str| serde_json::to_string(v).unwrap();
        let mut lines = vec!["{".to_string()];
        for (i, (a, (k, s))) in ACTIONS.iter().zip(&self.binds).enumerate() {
            lines.push(format!("  {}: {{", text(a.id)));
            lines.push(format!("    \"Name\": {},", text(a.name)));
            lines.push(format!("    \"Key\": {},", text(k)));
            lines.push(format!("    \"SecondaryKey\": {},", text(s)));
            lines.push(format!("    \"InvisibleInMenu\": {},", a.invisible));
            lines.push(format!("    \"Category\": {},", text(a.category)));
            lines.push(format!("    \"Universe\": {}", text(a.universe)));
            lines.push(format!("  }}{}", if i + 1 == ACTIONS.len() { "" } else { "," }));
        }
        lines.push("}".to_string());
        lines.join("\r\n")
    }

    /// ControlSchemeEditorBehaviour.LoadInputSystem: InputSystem.Load reads the saved actions over the defaults (the
    /// KeyCodes as numbers, or as names).
    pub fn load(&mut self, text: &str) {
        let Ok(serde_json::Value::Object(obj)) = serde_json::from_str(text) else { return };
        for (i, a) in ACTIONS.iter().enumerate() {
            let name = |f: &str| -> Option<String> {
                let v = obj.get(a.id)?.get(f)?;
                let n = match v.as_i64() {
                    Some(n) => crate::game::keycode::name(i32::try_from(n).ok()?)?,
                    None => v.as_str().filter(|n| *n == "None" || super::keys::src(n).is_some())?,
                };
                Some(n.to_string())
            };
            if let (Some(k), Some(s)) = (name("Key"), name("SecondaryKey")) {
                self.binds[i] = (k, s);
            }
        }
    }
}

/// What the game sees of a control: only presses that began where the world may take input go through, until
/// they end.
pub fn gate(owned: &mut bool, raw: Control, allowed: bool) -> Control {
    if raw.down && allowed {
        *owned = true;
    }
    if !*owned {
        return Control::default();
    }
    // Over once it is released, or the world no longer takes input.
    let up = raw.up || !allowed || (!raw.held && !raw.down);
    *owned = raw.held && !up;
    Control { down: raw.down, held: raw.held && allowed, up }
}

/// Utils.SmoothApproach
pub fn smooth_approach(past: f32, target: f32, speed: f32, dt: f32) -> f32 {
    crate::game::fire::smooth_approach(past, target, speed, dt)
}

/// Global.Update's time scale for this frame: 0 paused (Paused or the pause menu), else timeScaleTarget.
pub fn time_scale_target(stopped: bool, target: f32) -> f32 {
    if stopped { 0.0 } else { target }
}

/// A temperature in degrees Celsius, in the chosen TemperatureUnit (0 Celsius, 1 Fahrenheit, 2 Kelvin).
pub fn temperature_text(celsius: f32, unit: usize) -> String {
    match unit {
        1 => format!("{} F", ((celsius * 9.0 / 5.0 + 32.0) * 100.0).round() / 100.0),
        2 => format!("{} K", ((celsius + 273.15) * 100.0).round() / 100.0),
        _ => format!("{celsius} C"),
    }
}

// ---------------------------------------------------------------- navigation

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Main,
    Maps,
    Settings,
    Controls,
    Info,
    /// "Open-source licenses".
    Licenses,
    Mods,
    Stats,
    /// Rewrite feature: local achievements (no Steam).
    Achievements,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Menu,
    Loading,
    Game,
    Pause,
}

/// What the buttons and keys ask for.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Nav(Page),
    /// The crash screen's Report button.
    CrashReport,
    Back,
    /// Esc / the "pause" action.
    Escape,
    SelectMap(usize),
    Enter,
    Quit,
    Resume,
    Restart,
    ToMenu,
    /// Click into the catalog's filter box.
    Filter,
    /// The filter box's X.
    ClearFilter,
    /// Click into the Mods page's filter box (its TMP_InputField; SetFilter has no target, so the list never changes).
    ModsFilter,
    /// The Mods page filter's reset button: TMP_InputField.set_text("").
    ModsFilterReset,
    /// "Enable all " / "Disable all": ModListBehaviour.SetAllModStatus on the uninstantiated legacy prefab (no entries).
    ModsSetAll(bool),
    /// URLOpenBehaviour.OpenLocal("\\Mods").
    ModsOpenFolder,
    /// URLOpenBehaviour.OpenURL("https://www.studiominus.nl/ppg-modding/").
    ModsOpenDocs,
    /// A toolbar icon: the tool's name and its type.
    Tool(String, String),
    /// Settings tab: a `CATEGORIES` index.
    Tab(usize),
    Slowmo,
    /// Global.ToggleLimbStatus
    LimbStatus,
    TimeToggle,
    Category(usize),
    Item(String),
    /// A saved contraption's button: its index in `Catalog::items`.
    Contraption(usize),
    /// DeleteContraptionButtonBehaviour.CreateDialogBoxAndRemove of the contraption button (its `Catalog::items` index).
    DeleteContraption(usize),
    /// ItemButtonBehaviour.ShowInfoDialog and OpenWorkshop, by `Catalog::items` index.
    ItemInfo(usize),
    ItemWorkshop(usize),
    /// PublishButtonBehaviour.Publish of a contraption button.
    ItemPublish(usize),
    Powers(bool),
    /// The eye button: ToolWheelBehaviour.ToggleToolVisibility.
    ToolVisibility,
    /// Environment options' GameObject.SetActive: true from the catalog's Environment button, false from its Back button.
    Environment(bool),
    /// The environment window's checkbox for an `ENVIRONMENT` row, and a click into its number field.
    EnvToggle(usize),
    EnvEdit(usize),
    /// Enter or a click elsewhere ends the edit (onEndEdit).
    EnvEnd,
    /// The layer editor's arrows: (layer rather than order, up rather than down) -> LayerEditorController.RawOffsetLayer / RawOffsetOrder.
    LayerOffset(bool, bool),
    /// The map options' "Clear Mess" and "Clear Living" (the game runs them).
    ClearMess,
    ClearLiving,
    ClearEverything,
    Set(usize, f32),
    Rebind(usize),
    Unbind(usize),
    /// A context menu dynamic button: its index in `Ui::context_buttons`.
    Menu(usize),
    /// A context menu static button: its index in CONTEXT_MENU.
    MenuStatic(usize),
    /// The entry box's Apply and Cancel.
    EntryApply,
    EntryCancel,
    /// The Information page's Export game debug info button, and a button of the dialog it opens.
    ExportDebugInfo,
    DialogButton(usize),
    /// A button of a box under the top one (`under` index, button): that box comes to the top first.
    DialogUnder(usize, usize),
    /// The colour picker's Apply and Cancel, and a click on its hex code field.
    ColourApply,
    ColourCancel,
    ColourHex,
    /// The top key entry box: its key button (StartWaiting), Apply and Cancel.
    KeyWait,
    KeyApply,
    KeyCancel,
    /// The limb status view's close button (SetActive(false)).
    LimbViewClose,
    /// The piano dialog: a key (piano_keys index), Apply and Cancel.
    PianoKey(usize),
    /// UISoundBehaviour's listener on the clicked Button (Blip), Toggle (Blip) or Slider (SliderCallback).
    UiSound(crate::game::audio::UiSoundKind),
    PianoApply,
    PianoCancel,
}

/// An open DialogBox with DialogButtons: its title, SetHeight, and each button's text, ClosesDialogBox and delegate.
#[derive(Clone, Debug, PartialEq)]
pub struct DialogView {
    pub title: String,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub buttons: Vec<(String, bool, Option<DialogRun>)>,
    /// InputField.text of a box with ShowTextBox; None without one.
    pub entered: Option<String>,
}

impl DialogView {
    /// DialogBoxManager.Notification: the text as the title and one OK button that closes the box.
    pub fn notification(text: impl Into<String>) -> Self {
        DialogView { title: text.into(), width: None, height: None, buttons: vec![("OK".into(), true, None)], entered: None }
    }

    /// DialogBox.SetTitle: Title = title and the text mesh's text.
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    /// DialogBox.SetWidth
    pub fn set_width(&mut self, w: f32) {
        self.width = Some(w);
        super::uifn("DialogBox.SetWidth", || w.to_string());
    }

    /// DialogBox.SetHeight
    pub fn set_height(&mut self, h: f32) {
        self.height = Some(h);
        super::uifn("DialogBox.SetHeight", || h.to_string());
    }

    /// DialogBox.get_EnteredText: the input field's text with ShowTextBox, else "".
    pub fn entered_text(&self) -> &str {
        self.entered.as_deref().unwrap_or("")
    }

    /// DialogBox.set_EnteredText: the input field's text; a box without a text box throws.
    pub fn set_entered_text(&mut self, value: impl Into<String>) {
        match &mut self.entered {
            Some(t) => *t = value.into(),
            None => panic!("{}", crate::il!(str "DialogBox::set_EnteredText" 0)),
        }
    }
}

/// DialogButton..ctor(label, closesDialogBox, params action): Label, ClosesDialogBox and Actions.
pub fn dialog_button(label: &str, closes_dialog_box: bool, action: Option<DialogRun>) -> (String, bool, Option<DialogRun>) {
    (label.into(), closes_dialog_box, action)
}

/// The delegates ExportGameDebugInfoBehaviour.Export gives its buttons (the full path of the file).
#[derive(Clone, Debug, PartialEq)]
pub enum DialogRun {
    ShowInExplorer(String),
    OpenFile(String),
    /// ItemButtonBehaviour.ShowInfoDialog's Open in Explorer: the folder of the contraption's data file.
    OpenFolder(String),
    /// ItemButtonBehaviour.OnPointerDown's Select anyway: the contraption button (its `Catalog::items` index) loaded.
    SelectContraption(usize),
    /// A contraption dialog's delegate (contraption.rs).
    Game(crate::game::contraption::DialogAction),
}

/// What a transition needs the world for.
#[derive(Debug, PartialEq)]
pub enum Effect {
    None,
    Load(String),
    Quit,
    ToMenu,
}

pub struct MapRow {
    pub name: String,
    pub description: String,
}

/// A TMP_InputField on the environment window being edited: its `ENVIRONMENT` row, its text, whether the text is all selected,
/// and the text it had when focus came (Escape restores it).
#[derive(Clone, Debug)]
pub struct EnvEdit {
    pub row: usize,
    pub text: String,
    pub all: bool,
    pub original: String,
}

#[derive(Resource)]
pub struct Ui {
    /// The Stats page's UIStatViewBehaviour ValueDisplay texts, by `STAT_TILES`.
    pub stat_values: Vec<String>,
    /// Camera.main's pixelWidth and pixelHeight.
    pub screen: [f32; 2],
    pub mode: Mode,
    pub pages: Vec<Page>,
    pub tab: usize,
    pub maps: Vec<MapRow>,
    /// The Menu scene's loading overlay while active: its TitleMesh and StateMesh texts (BackgroundItemLoaderStatusBehaviour.Update).
    pub loading_status: Option<(String, String, bool)>,
    pub selected: Option<usize>,
    last_click: (usize, f64),
    pub map: Option<String>,
    /// The installed game's folder.
    pub game: std::path::PathBuf,
    pub catalog: super::data::Catalog,
    pub paused: bool,
    pub slowmo: bool,
    /// Global.timeScaleTarget: 1, or SlowMotionSpeed / 100 as of the last ToggleSlowmotion.
    pub timeScaleTarget: f32,
    pub powers: bool,
    /// The chosen tool's name.
    pub tool: String,
    pub category: usize,
    pub filter: String,
    /// The filter box's text is what the list shows (CatalogBehaviour.SetFilter ran last), not the category (SetCategory ran last).
    pub filter_applied: bool,
    /// Keys go to the filter box.
    pub typing: bool,
    /// The Mods page filter box's text, and whether keys go to it.
    pub mods_filter: String,
    pub mods_typing: bool,
    pub item: Option<String>,
    /// CatalogBehaviour's selected contraption, outline and mods.
    pub local: super::catalog::LocalCatalog,
    pub env_open: bool,
    /// The map's EnvironmentalSettings as of the last frame (mirrors the game's, for the window's controls).
    pub env_values: Option<crate::game::map::EnvironmentalSettings>,
    /// The environment number field being edited: its row, its text and whether the text is all selected.
    pub env_edit: Option<EnvEdit>,
    /// LayerEditorController's open Container: its screen position and the LayerDisplay and OrderDisplay texts.
    pub layer: Option<([f32; 2], String, String)>,
    pub hide_panels: bool,
    /// The UI used this frame's "pause" press itself (InvokeOnEscape's Back, closing the context menu): Global.Update does not see it.
    pub pause_taken: bool,
    pub hide_ui: bool,
    pub context: Option<[f32; 2]>,
    /// ContextMenuInteractableConditionalBehaviour: each static button interactable.
    pub context_enabled: Vec<bool>,
    /// The Follow Status item's text while it is active (FollowingViewBehaviour).
    pub follow: Option<String>,
    /// ConditionalTextBehaviour: the text last set on each static button.
    pub context_labels: Vec<Option<&'static str>>,
    /// The open context menu's dynamic buttons (ContextMenuBehaviour.Show).
    pub context_buttons: Vec<crate::game::menu::ShownButton>,
    /// The open number entry box (DialogBoxManager.TextEntry): a copy whose text the keys edit until Apply.
    pub entry: Option<crate::game::menu::EntryBox>,
    /// An open DialogBoxManager.Dialog or Notification (ExportGameDebugInfoBehaviour.Export): the newest, on top.
    pub dialog: Option<DialogView>,
    /// The boxes opened before it and still open, oldest first (DialogBoxManager opens a box over any others).
    pub under: Vec<DialogView>,
    /// SteamWarningDialog's t while the Menu scene is up (None once it is disabled), and its static alreadyShown.
    pub steam_t: Option<f32>,
    pub steam_shown: bool,
    /// The Loading page is the Main scene's (under its Pause screen) rather than the Menu scene's.
    pub loading_in_main: bool,
    /// The scene load's AsyncOperation.progress, as SceneSwitchBehaviour.UpdateProgress last passed it to SetProgress.
    pub load_progress: f32,
    /// The entry box's input field has all its text selected (onFocusSelectAll, until the first edit).
    pub entry_all: bool,
    /// The open colour picker: a copy the drags and keys edit until Apply.
    pub colour: Option<crate::game::menu::ColourBox>,
    /// The game's open key entry boxes as last drawn.
    pub key_boxes: Vec<crate::game::menu::KeyBox>,
    /// The entry box, key boxes and colour picker open last frame, and a close whose OpenDialogboxCount change is still
    /// to record (true: a DialogBox, whose OnDestroy comes too).
    pub boxes_prev: (bool, usize, bool),
    pub box_closed: Option<bool>,
    /// The game's limb status view while active: its texts and bars.
    pub limb_view: Option<([String; 3], String, [f32; 4])>,
    /// The catalog's ResizableUIElementHandle.ToResize width once the handle has set it (else ToyboxSizeOffset is).
    pub toybox: Option<f32>,
    /// ResizableUIElementHandle.isBeingDragged: initialSize, initialMousePos.x (physical pixels).
    pub resizing: Option<(f32, f32)>,
    /// The game's piano dialog as last drawn.
    pub piano: Option<crate::game::menu::Piano>,
    /// Global.main.ShowLimbStatus (LimbStatusToggle's Visible).
    pub detail: bool,
    /// StatusController.Update: ThermalVisionBehaviour.ThermalVisionEnabled and ToolWheelBehaviour.ToolVisibility == Invisible.
    pub thermal: bool,
    pub tools_invisible: bool,
    pub rebinding: Option<usize>,
    /// NotificationControllerBehaviour.notificationObjects: each notification's text and the real time it was made.
    pub notes: Vec<(String, f64)>,
    pub error: Option<String>,
    pub dirty: bool,
    pub scroll: HashMap<u8, f32>,
}

impl Ui {
    /// NotificationControllerBehaviour.CreateNotification (MaximumAllowedNotifications 8 in the Main scene): past the
    /// limit the oldest goes first; each is destroyed NotificationLifetime real seconds later (notes_update).
    pub fn notify(&mut self, text: impl Into<String>, now: f64) {
        if self.notes.len() > 8 {
            self.notes.remove(0);
        }
        self.notes.push((text.into(), now));
    }

    pub fn new(maps: Vec<MapRow>, mode: Mode, map: Option<String>, item: Option<String>) -> Self {
        // CatalogBehaviour.Awake: Main = this (this resource is the one catalog); the ResizableUIElementHandle is layout.
        // LocalCatalogBehaviour.Start: calls RefreshContraptionButtons, whose body is empty.
        // LocalCatalogBehaviour.RefreshContraptionButtons: empty.
        Self {
            mode,
            pages: vec![Page::Main],
            tab: 0,
            maps,
            loading_status: None,
            screen: [1920.0, 1080.0],
            selected: None,
            last_click: (usize::MAX, f64::MIN),
            map,
            paused: false,
            slowmo: false,
            timeScaleTarget: 1.0,
            powers: false,
            category: 0,
            filter: String::new(),
            filter_applied: false,
            typing: false,
            mods_filter: String::new(),
            mods_typing: false,
            item,
            local: Default::default(),
            game: Default::default(),
            catalog: Default::default(),
            tool: "Drag".into(),
            env_open: false,
            env_values: None,
            env_edit: None,
            layer: None,
            hide_panels: false,
            pause_taken: false,
            hide_ui: false,
            context: None,
            context_buttons: Vec::new(),
            context_enabled: Vec::new(),
            follow: None,
            context_labels: Vec::new(),
            entry: None,
            dialog: None,
            under: Vec::new(),
            steam_t: Some(0.0),
            steam_shown: false,
            loading_in_main: false,
            load_progress: 0.0,
            entry_all: false,
            toybox: None,
            resizing: None,
            colour: None,
            key_boxes: Vec::new(),
            boxes_prev: (false, 0, false),
            box_closed: None,
            limb_view: None,
            piano: None,
            detail: false,
            thermal: false,
            tools_invisible: false,
            rebinding: None,
            notes: Vec::new(),
            error: None,
            dirty: true,
            scroll: HashMap::new(),
            stat_values: Vec::new(),
        }
    }

    /// The scene load's UIStatViewBehaviours: MenuController.Awake activates the Stats page (OnEnable: UpdateValue),
    /// then Start. In Main the Pause screen is deactivated before Start (Global.Start's SetPausedMenu(false)), so Start
    /// runs at the first pause; none of Main's tiles has GetFromSteam and both of its calls are UpdateValue there.
    pub fn stat_views_load(&mut self, scene: &str) {
        // A run without the game's scenes (the unit tests) has no tiles.
        if !super::scene::has(scene, &stat_tile_path(scene, STAT_TILES[0])) {
            return;
        }
        self.stat_views_enable(scene);
        for tile in STAT_TILES {
            stat_view_start(scene, tile);
        }
        self.stat_views_enable(scene);
    }

    /// UIStatViewBehaviour.OnEnable: UpdateValue on each tile.
    pub fn stat_views_enable(&mut self, scene: &str) {
        if !super::scene::has(scene, &stat_tile_path(scene, STAT_TILES[0])) {
            return;
        }
        self.stat_values = STAT_TILES.iter().map(|t| stat_view_value(scene, t)).collect();
        self.dirty = true;
    }

    /// What the catalog list is filtered by: the filter box's text once it was edited after the last category click.
    pub fn list_filter(&self) -> &str {
        if self.filter_applied { &self.filter } else { "" }
    }

    /// A map has been loaded and the game starts.
    pub fn started(&mut self) {
        // Global.Start: ActiveUiBlockers = 0 (none are modelled), SetPausedMenu(false), timeScaleTarget = 1; its UpdatePitch(1) matches the new Game (sources registered at timeScale 1)
        self.mode = Mode::Game;
        // EnvironmentSettingsController.Awake (Main is the window) and Start (the window closed, its rows regenerated).
        super::uifn("EnvironmentSettingsController.Awake", || "True".into());
        // ColorpickerDialogBehaviour.Awake (Instance is the box) and Start (the box closed).
        super::uifn("ColorpickerDialogBehaviour.Awake", || "True".into());
        super::uifn("ColorpickerDialogBehaviour.Start", || "False".into());
        super::uifn("EnvironmentSettingsController.Start", || format!("False {}", super::tables::ENVIRONMENT.len()));
        super::ENV_REGEN.store(true, std::sync::atomic::Ordering::Relaxed);
        self.pages = vec![Page::Main];
        self.paused = false;
        self.slowmo = false;
        self.timeScaleTarget = 1.0;
        self.context = None;
        self.dirty = true;
        // CatalogBehaviour.Start (the Main scene's catalog): the categories, SelectedCategory the first, the item buttons.
        // ContextMenuBehaviour.Awake: Instance is this one (`ui.context`), and the canvas it scales with (view::CANVAS_WIDTH
        // wide, matching the width).
        let state = format!("{} {} {} {}", self.catalog.categories.len(), self.selected_category(), self.items().len(), self.contraptions().count());
        super::uifn("CatalogBehaviour.Start", || state);
        super::uifn("ContextMenuBehaviour.Awake", || format!("True {} 0", super::view::CANVAS_WIDTH));
        super::uifn("DialogBoxManager.Awake", || format!("True {}", super::view::DIALOG_CONTAINER));
    }

    /// CatalogBehaviour.get_Items: every item button, the spawnables' and the saved contraptions' (CreateItemButtons adds both).
    pub fn items(&self) -> &[super::data::Item] {
        let n = self.catalog.items.len();
        super::uifn("CatalogBehaviour.get_Items", || n.to_string());
        &self.catalog.items
    }

    pub fn failed(&mut self, error: String) {
        self.mode = Mode::Menu;
        self.pages = vec![Page::Main, Page::Maps];
        self.error = Some(error);
        self.dirty = true;
    }

    fn pop(&mut self) {
        if self.pages.len() > 1 {
            self.pages.pop();
            self.page_selected();
        }
    }

    /// MenuController.set_CurrentPage: the page that becomes current gets OnPageSelected (MenuController.SetPage is `pages`).
    /// SettingsTabsController.OnPageSelected, SetCategory(StartingCategory, 0 in every scene: General) and OnNextFrame: the tab
    /// goes back to General and the list to its top (ScrollRect.verticalNormalizedPosition = 1).
    fn page_selected(&mut self) {
        // MenuController.Awake: the first page is current (Ui::new's pages) and every page is active
        // MenuController.SetPage: the page by name becomes current, a name with no page changes nothing
        // MenuController.GetPageByName: the Page of that name
        // MenuController.get_CurrentPage: the last of `pages`
        // SettingsTabsController.OnPageSelected: SetCategory(StartingCategory) after a frame
        // SettingsTabsController.OnNextFrame: the frame's end (the rebuild)
        // SettingsTabsController.Start: SetCategory(StartingCategory), the tab a rebuilt Ui starts on
        if self.pages.last() == Some(&Page::Settings) {
            self.tab = 0;
            self.scroll.insert(1, 0.0);
        }
        // ModListUiBehaviour.OnPageSelected: ResetView
        // ModListUiBehaviour.ResetView: at the frame's end the list scrolls to its top (verticalNormalizedPosition 1)
        if self.pages.last() == Some(&Page::Mods) {
            self.scroll.insert(5, 0.0);
        }
    }

    /// The transitions that need no world. `now` is in seconds (double clicks).
    /// DialogBoxManager's new box: over any open ones.
    pub fn open_dialog(&mut self, d: DialogView) {
        if let Some(old) = self.dialog.replace(d) {
            self.under.push(old);
        }
        // DialogBox.Start: OpenDialogboxCount++.
        let n = self.open_dialogbox_count();
        super::uifn("DialogBox.set_OpenDialogboxCount", || n.to_string());
    }

    /// DialogBox.Close of the top box: the one under it is the top again. The object goes at the frame's end, then
    /// OnDestroy takes it from OpenDialogboxCount.
    pub fn close_dialog(&mut self) {
        let n = self.open_dialogbox_count();
        if let Some(d) = &self.dialog {
            let title = super::ui_text(&d.title);
            super::uifn("DialogBox.Close", || format!("{n} {title}"));
        }
        self.dialog = self.under.pop();
        let n = self.open_dialogbox_count();
        super::uifn("DialogBox.set_OpenDialogboxCount", || n.to_string());
        super::uifn("DialogBox.OnDestroy", || n.to_string());
    }

    /// DialogBox.OpenDialogboxCount: DialogBox.Start adds one and OnDestroy takes one (the colour picker too, as it opens
    /// and closes): the dialogs and notifications, the text entry, the key entry boxes and the colour picker open.
    pub fn open_dialogbox_count(&self) -> usize {
        let n = usize::from(self.dialog.is_some()) + self.under.len() + usize::from(self.entry.is_some()) + usize::from(self.colour.is_some()) + self.key_boxes.len();
        super::uifn("DialogBox.get_OpenDialogboxCount", || n.to_string());
        n
    }

    /// DialogBox.IsAnyDialogboxOpen
    pub fn is_any_dialogbox_open(&self) -> bool {
        let any = self.open_dialogbox_count() > 0;
        super::uifn("DialogBox.get_IsAnyDialogboxOpen", || super::cs(any).into());
        any
    }

    pub fn press(&mut self, act: &Act, now: f64) -> Effect {
        self.dirty = true;
        crate::crash::note(format!("{act:?}"));
        match act {
            Act::CrashReport => crate::crash::open_report(),
            Act::Nav(p) => {
                // Opened from the toolbar, a page belongs to the pause menu.
                if self.mode == Mode::Game {
                    self.mode = Mode::Pause;
                    self.pages = vec![Page::Main];
                    self.stat_views_enable("Main");
                }
                self.pages.push(*p);
                self.page_selected();
                self.error = None;
            }
            Act::Back => self.pop(),
            // Global.SetPausedMenu: pausedMenu is Mode::Pause, and MenuController.SetPage("Main") is `pages`; the UI refresh around it is presentation
            // Global.OpenPauseMenu: SetPausedMenu(true) (Escape in the game)
            // Global.ClosePauseMenu: SetPausedMenu(false) (Escape or Resume in the pause menu)
            Act::Escape => match self.mode {
                Mode::Game if self.context.take().is_some() => {}
                Mode::Game => {
                    self.mode = Mode::Pause;
                    self.pages = vec![Page::Main];
                    self.stat_views_enable("Main");
                }
                Mode::Pause if self.pages.len() > 1 => self.pop(),
                Mode::Pause => {
                    self.mode = Mode::Game;
                    super::ENV_REGEN.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                Mode::Menu => self.pop(),
                Mode::Loading => {}
            },
            Act::SelectMap(i) => {
                // MapViewBehaviour.Select: SelectedMap = Map, then a second click within DoubleClickTime (0.2 s) enters it.
                self.selected = Some(*i);
                let double = self.last_click.0 == *i && now - self.last_click.1 < 0.2;
                self.last_click = (*i, now);
                if double {
                    return self.press(&Act::Enter, now);
                }
            }
            Act::Enter => {
                // PlayMapButtonBehaviour.Switch: CurrentMap = SelectedMap; the menu's SceneSwitchBehaviour.Switch is the Loading page.
                // SceneSwitchBehaviour.Switch: MenuController.SetPage("Loading") and the scene load (Effect::Load).
                if let Some(m) = self.selected.and_then(|i| self.maps.get(i)) {
                    self.map = Some(m.name.clone());
                    // MapLoaderBehaviour.Awake (Load) in the loaded scene: SelectedMap = null.
                    self.selected = None;
                    self.loading_in_main = self.mode == Mode::Pause;
                    self.mode = Mode::Loading;
                    return Effect::Load(m.name.clone());
                }
            }
            Act::Restart => {
                if let Some(m) = self.map.clone() {
                    self.loading_in_main = true;
                    self.mode = Mode::Loading;
                    self.selected = None;
                    return Effect::Load(m);
                }
            }
            Act::Resume => {
                self.mode = Mode::Game;
                super::ENV_REGEN.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            Act::ToMenu => {
                self.mode = Mode::Menu;
                self.pages = vec![Page::Main];
                self.stat_views_load("Menu");
                self.selected = None;
                return Effect::ToMenu;
            }
            Act::Quit => return Effect::Quit,
            // SettingsTabsController.SetCategoryGeneral: SetCategory shows the tab's settings and scrolls the list to the top
            // SettingsTabsController.SetCategoryGore: the same
            // SettingsTabsController.SetCategoryVisualEffects: the same
            // SettingsTabsController.SetCategoryUserInterface: the same
            // SettingsTabsController.SetCategoryVideo: the same
            // SettingsTabsController.SetCategoryAudio: the same
            // SettingsTabsController.SetCategory: the same, for the tab's own category
            Act::Tab(t) => {
                self.tab = *t;
                self.scroll.insert(1, 0.0);
            }
            // Global.ToggleSlowmotion: SlowMotion flips and says so; timeScaleTarget is set where the preferences are (mod.rs).
            Act::Slowmo => {
                self.slowmo = !self.slowmo;
                self.notify(if self.slowmo { crate::il!(str "Global::ToggleSlowmotion" 1) } else { crate::il!(str "Global::ToggleSlowmotion" 0) }, now);
            }
            // Global.TogglePaused: Paused flips and says so. Its DelayCoroutine is never started (Utils.DelayCoroutine only returns the
            // IEnumerator), so the Physics2D.Simulate it holds never runs. The rigidbody interpolation mode is a render setting.
            Act::TimeToggle => {
                self.paused = !self.paused;
                self.notify(if self.paused { "Paused" } else { "Unpaused" }, now);
            }
            // CatalogBehaviour.SetCategory: the category is one of the catalog's; the rebuildUi coroutine puts the item list's scroll
            // back to the top (verticalNormalizedPosition = 1; the layout rebuilds are presentation).
            // CatalogBehaviour.set_SelectedCategory: the list shows that category's items (`shown`); the filter box keeps its text.
            Act::Category(c) => {
                self.category = *c;
                self.filter_applied = false;
                self.scroll.insert(3, 0.0);
                self.typing = false;
            }
            // ItemButtonBehaviour.OnPointerDown: for a SpawnableAsset (no dialog box is open) it is CatalogBehaviour.SetItem.
            // CatalogBehaviour.SetItem: the item is in spawnableSet (every button's is), SelectedContraption = null (none exist).
            // CatalogBehaviour.add_SelectionChanged: ToyStatusView's Start subscribes for the scene's life; it reads `item` when drawn.
            // CatalogBehaviour.SetItem: the spawnable selected, no contraption.
            Act::Item(n) => {
                self.local.selected = None;
                self.local.changed = true;
                self.item = Some(n.clone());
                let state = format!("{} {}", super::ui_text(n), super::cs(self.local.selected.is_none()));
                super::uifn("CatalogBehaviour.SetItem", || state);
            }
            Act::Contraption(i) => self.select_contraption(*i, now),
            Act::ItemInfo(i) => {
                if let Some(item) = self.catalog.items.get(*i) {
                    if let Some(d) = super::catalog::ItemButton::of(item).show_info_dialog(self.screen) {
                        self.open_dialog(d);
                    }
                }
            }
            Act::ItemWorkshop(i) => {
                if let Some(item) = self.catalog.items.get(*i) {
                    super::catalog::ItemButton::of(item).open_workshop();
                }
            }
            Act::Filter => self.typing = !self.typing,
            Act::ModsFilter => self.mods_typing = !self.mods_typing,
            Act::ModsFilterReset => self.mods_filter.clear(),
            // ModListBehaviour.SetAllModStatus: its modEntries are empty on the prefab asset the buttons target.
            Act::ModsSetAll(_) => {}
            Act::ModsOpenFolder => url_open_behaviour_open_local(r"\\Mods"),
            Act::ModsOpenDocs => super::debug_info::open_url("https://www.studiominus.nl/ppg-modding/"),
            Act::ClearFilter => {
                self.filter.clear();
                self.filter_applied = false;
                self.typing = false;
            }
            Act::Tool(name, _) => self.tool = name.clone(),
            Act::Powers(p) => self.powers = *p,
            Act::Environment(open) => self.env_open = *open,
            // Settings and control edits are applied where the stores live.
            Act::DeleteContraption(_) | Act::ItemPublish(_) | Act::EnvToggle(_) | Act::EnvEdit(_) | Act::EnvEnd | Act::LayerOffset(..) | Act::Set(..) | Act::Rebind(_) | Act::Unbind(_) | Act::ClearMess | Act::ClearLiving | Act::ClearEverything | Act::Menu(_) | Act::MenuStatic(_) | Act::EntryApply | Act::EntryCancel | Act::ExportDebugInfo | Act::DialogButton(_) | Act::DialogUnder(..) | Act::ColourApply | Act::ColourCancel | Act::ColourHex | Act::KeyWait | Act::KeyApply | Act::KeyCancel | Act::LimbViewClose | Act::PianoKey(_) | Act::UiSound(_) | Act::PianoApply | Act::PianoCancel | Act::LimbStatus | Act::ToolVisibility => {}
        }
        Effect::None
    }
}

/// URLOpenBehaviour.OpenLocal: Path.GetFullPath, then Process.Start of it (shell execute) on Windows, xdg-open on Linux,
/// open on macOS. The Mods page passes "\\Mods", which GetFullPath keeps as a UNC path (\\Mods), not the game's folder.
pub fn url_open_behaviour_open_local(path: &str) {
    use std::process::Command;
    let Ok(full) = std::path::absolute(path) else { return };
    let _ = if cfg!(windows) {
        Command::new("cmd").args(["/c", "start", ""]).arg(&full).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new(crate::il!(str "URLOpenBehaviour::OpenLocal" 1)).arg(&full).spawn()
    } else {
        Command::new(crate::il!(str "URLOpenBehaviour::OpenLocal" 0)).arg(&full).spawn()
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> Ui {
        let rows = ["Default", "Arena"].map(|n| MapRow { name: n.into(), description: String::new() });
        Ui::new(rows.into(), Mode::Menu, None, None)
    }

    #[test]
    fn play_select_enter_loads_the_map() {
        let mut ui = menu();
        assert_eq!(ui.press(&Act::Enter, 0.0), Effect::None, "nothing selected");
        ui.press(&Act::Nav(Page::Maps), 0.0);
        ui.press(&Act::SelectMap(1), 1.0);
        assert_eq!(ui.press(&Act::Enter, 2.0), Effect::Load("Arena".into()));
        assert_eq!(ui.mode, Mode::Loading);
        ui.started();
        assert_eq!(ui.mode, Mode::Game);
        // A failed load goes back to the map list with the message.
        ui.failed("boom".into());
        assert_eq!((ui.mode, ui.pages.last().copied(), ui.error.as_deref()), (Mode::Menu, Some(Page::Maps), Some("boom")));
    }

    #[test]
    fn double_click_enters() {
        let mut ui = menu();
        ui.press(&Act::Nav(Page::Maps), 0.0);
        assert_eq!(ui.press(&Act::SelectMap(0), 1.0), Effect::None);
        assert_eq!(ui.press(&Act::SelectMap(0), 1.1), Effect::Load("Default".into()));
        let mut ui = menu();
        ui.press(&Act::SelectMap(0), 1.0);
        assert_eq!(ui.press(&Act::SelectMap(0), 1.5), Effect::None, "too slow");
    }

    #[test]
    fn settings_page_selected_resets_the_tab() {
        let mut ui = menu();
        ui.press(&Act::Nav(Page::Settings), 0.0);
        ui.press(&Act::Tab(2), 0.0);
        ui.scroll.insert(1, 300.0);
        ui.press(&Act::Tab(3), 0.0);
        assert_eq!((ui.tab, ui.scroll[&1]), (3, 0.0), "SetCategory scrolls to the top");
        ui.press(&Act::Nav(Page::Controls), 0.0);
        ui.press(&Act::Back, 0.0);
        assert_eq!(ui.tab, 0, "OnPageSelected: back to the starting category");
    }

    #[test]
    fn escape_walks_back_and_pauses() {
        let mut ui = menu();
        ui.press(&Act::Nav(Page::Settings), 0.0);
        ui.press(&Act::Nav(Page::Controls), 0.0);
        ui.press(&Act::Escape, 0.0);
        assert_eq!(ui.pages, [Page::Main, Page::Settings]);
        ui.press(&Act::Back, 0.0);
        ui.press(&Act::Escape, 0.0);
        assert_eq!((ui.mode, ui.pages.len()), (Mode::Menu, 1), "Esc at the main menu stays");
        ui.started();
        ui.press(&Act::Escape, 0.0);
        assert_eq!(ui.mode, Mode::Pause);
        ui.press(&Act::Nav(Page::Settings), 0.0);
        ui.press(&Act::Escape, 0.0);
        assert_eq!((ui.mode, ui.pages.len()), (Mode::Pause, 1));
        ui.press(&Act::Resume, 0.0);
        ui.press(&Act::Nav(Page::Settings), 0.0);
        assert_eq!((ui.mode, ui.pages.clone()), (Mode::Pause, vec![Page::Main, Page::Settings]), "the toolbar's Settings button");
        ui.press(&Act::Back, 0.0);
        ui.press(&Act::Escape, 0.0);
        assert_eq!(ui.mode, Mode::Game);
        // Esc closes the context menu first.
        ui.context = Some([1.0, 2.0]);
        ui.press(&Act::Escape, 0.0);
        assert_eq!((ui.mode, ui.context), (Mode::Game, None));
    }

    #[test]
    fn pause_menu_actions() {
        let mut ui = menu();
        ui.map = Some("Default".into());
        ui.started();
        ui.press(&Act::TimeToggle, 0.0);
        ui.press(&Act::Slowmo, 0.0);
        assert!(ui.paused && ui.slowmo);
        assert_eq!(ui.press(&Act::Restart, 0.0), Effect::Load("Default".into()));
        ui.started();
        assert!(!ui.paused && !ui.slowmo, "a new game starts at full speed");
        assert_eq!(ui.press(&Act::ToMenu, 0.0), Effect::ToMenu);
        assert_eq!(ui.press(&Act::Quit, 0.0), Effect::Quit);
    }

    #[test]
    fn tooltips_end_in_punctuation() {
        assert_eq!(append_punctuation("  A gun  "), "A gun.");
        assert_eq!(append_punctuation("Really?"), "Really?");
        assert_eq!(append_punctuation("<b>x</b>\n(y)"), "<b>x</b>\n(y)");
        assert_eq!(tooltip("Pistol", "Fires bullets"), "Pistol\nFires bullets.");
        assert_eq!(tooltip("Pistol", "Fires bullets!"), "Pistol\nFires bullets!");
        assert_eq!(tooltip("Pistol", ""), "Pistol.");
    }

    #[test]
    fn catalog_selection_is_what_the_game_spawns() {
        let mut ui = menu();
        ui.press(&Act::Item("Pistol".into()), 0.0);
        assert_eq!(ui.item.as_deref(), Some("Pistol"));
        ui.filter = "x".into();
        ui.filter_applied = true;
        ui.scroll.insert(3, 80.0);
        ui.press(&Act::Category(2), 0.0);
        assert_eq!((ui.category, ui.filter.as_str(), ui.list_filter(), ui.scroll[&3]), (2, "x", "", 0.0));
        ui.press(&Act::Filter, 0.0);
        assert!(ui.typing);
        ui.press(&Act::Category(0), 0.0);
        assert!(!ui.typing);
    }

    #[test]
    fn settings_defaults_match_the_game_and_the_simulation() {
        let s = Settings::default();
        let d = Prefs::default();
        let p = s.prefs();
        assert_eq!(format!("{p:?}"), format!("{d:?}"));
        assert_eq!(s.get("SlowMotionSpeed"), 20.0);
        assert_eq!(s.get("WindowMode"), 1.0);
        assert!(SETTINGS.iter().all(|s| s.default.is_finite()));
    }

    #[test]
    fn settings_text_follows_the_format_attributes() {
        let mut s = Settings::default();
        let t = |s: &Settings, f: &str| s.text(Settings::index(f));
        assert_eq!(t(&s, "PhysicsIterations"), "32 iterations");
        assert_eq!(t(&s, "ShakeIntensity"), "1x");
        assert_eq!(t(&s, "GridSnap"), "0.25 m");
        assert_eq!(t(&s, "CollisionQuality"), "Dynamic");
        assert_eq!(t(&s, "ShowOutlines"), "Enabled");
        assert_eq!(t(&s, "MasterVolume"), "100%");
        assert_eq!(t(&s, "RenderScale"), "100%");
        s.set(Settings::index("CrushForceMultiplier"), 0.57);
        assert_eq!(t(&s, "CrushForceMultiplier"), "55%");
        s.set(Settings::index("RenderScale"), 0.6);
        assert_eq!(t(&s, "RenderScale"), "50%");
        s.set(Settings::index("AngleSnap"), 17.4);
        assert_eq!(t(&s, "AngleSnap"), "17 degrees");
    }

    #[test]
    fn settings_clamp_snap_and_round_trip() {
        let mut s = Settings::default();
        let i = Settings::index("PhysicsIterations");
        s.set(i, 1000.0);
        assert_eq!(s.get("PhysicsIterations"), 256.0);
        s.set(Settings::index("ThermalVisionUpdateRate"), 33.0);
        assert_eq!(s.get("ThermalVisionUpdateRate"), 35.0);
        s.set_fraction(Settings::index("Brightness"), 0.5);
        assert_eq!(s.get("Brightness"), 205.0);
        s.set(Settings::index("TemperatureUnit"), 2.0);
        s.set(Settings::index("GorelessMode"), 1.0);
        s.set(Settings::index("FragilityMultiplier"), 0.5);
        let mut t = Settings::default();
        t.load(&s.json());
        assert_eq!(t.values, s.values);
        assert!(t.prefs().GorelessMode);
        assert_eq!(t.prefs().FragilityMultiplier, 0.5);
        // The game's config.json: names or numbers for enums, unknown members ignored.
        let mut g = Settings::default();
        g.load(r#"{"CollisionQuality":"Continuous","ToolSelectionMode":1,"Resolution":{"x":1},"Brightness":"bad","GridSnap":0.5}"#);
        assert_eq!((g.get("CollisionQuality"), g.get("ToolSelectionMode"), g.get("GridSnap"), g.get("Brightness")), (1.0, 1.0, 0.5, 100.0));
    }

    #[test]
    fn menu_tabs_come_from_the_table() {
        let general: Vec<_> = tab_rows(0).map(|i| SETTINGS[i].field).collect();
        assert_eq!(general.first(), Some(&"CollisionQuality"));
        assert!(!general.contains(&"ShowTutorial"), "hidden fields stay out of the menu");
        assert_eq!(SETTINGS_MENU.iter().map(|e| e.1).collect::<Vec<_>>(), ["General", "Controls", "Gore", "VisualEffects", "UserInterface", "Audio", "Video", "Mods", "Back"]);
        for c in 0..CATEGORIES.len() {
            assert!(tab_rows(c).count() > 0, "{c}");
        }
    }

    #[test]
    fn controls_defaults_conflicts_and_files() {
        let mut c = Controls::default();
        assert_eq!(ACTIONS.len(), 57);
        assert_eq!(c.text(Controls::index("drag")), "Mouse0");
        assert_eq!(c.text(Controls::index("slowmo")), "G");
        let slowmo = Controls::index("slowmo");
        assert!(!c.conflicts(slowmo), "the map editor's G is another universe");
        c.set(slowmo, "Space", "None");
        assert!(c.conflicts(slowmo) && c.conflicts(Controls::index("time")));
        c.set(slowmo, "Space", "LeftShift");
        assert_eq!(c.text(slowmo), "LeftShift + Space");
        let mut d = Controls::default();
        d.load(&c.json());
        assert_eq!(d.binds, c.binds);
    }

    #[test]
    fn gate_passes_only_presses_that_began_in_the_world() {
        let held = |down, held, up| Control { down, held, up };
        let mut o = false;
        // Pressed over the UI: nothing reaches the game, not even later in the world.
        assert_eq!(gate(&mut o, held(true, true, false), false), Control::default());
        assert_eq!(gate(&mut o, held(false, true, false), true), Control::default());
        // Pressed in the world and then dragged over the UI: released.
        assert_eq!(gate(&mut o, held(true, true, false), true), held(true, true, false));
        assert_eq!(gate(&mut o, held(false, true, false), true), held(false, true, false));
        assert_eq!(gate(&mut o, held(false, true, false), false), held(false, false, true));
        assert_eq!(gate(&mut o, held(false, true, false), true), Control::default());
        // A normal release.
        gate(&mut o, held(true, true, false), true);
        assert_eq!(gate(&mut o, held(false, false, true), true), held(false, false, true));
    }

    #[test]
    fn time_scale_follows_the_games_smoothing() {
        assert_eq!(smooth_approach(1.0, 1.0, 15.0, 0.016), 1.0, "full speed stays exactly 1");
        assert_eq!(time_scale_target(false, 0.2), 0.2);
        assert_eq!(time_scale_target(true, 0.2), 0.0);
        let mut ts = 1.0;
        for _ in 0..60 {
            ts = smooth_approach(ts, 0.2, 15.0, 1.0 / 60.0);
        }
        assert!((ts - 0.2).abs() < 1e-3);
        assert_eq!(temperature_text(18.0, 1), "64.4 F");
        assert_eq!(temperature_text(18.0, 2), "291.15 K");
    }
}

#[cfg(test)]
mod save_tests {
    use super::*;

    /// InputSystem.Save's Json.NET text, and InputSystem.Load reading it back.
    #[test]
    fn controls_json_is_jsonnet_and_loads_back() {
        let mut c = Controls::default();
        let json = c.json();
        let lf = json.replace("\r\n", "\n");
        let head = "{\n  \"pan\": {\n    \"Name\": \"Pan\",\n    \"Key\": \"Mouse2\",\n    \"SecondaryKey\": \"None\",\n    \"InvisibleInMenu\": false,\n    \"Category\": \"Camera\",\n    \"Universe\": \"All\"\n  },\n";
        assert!(lf.starts_with(head), "{:?}", &lf[..150]);
        assert!(lf.ends_with("  }\n}"));
        c.set(0, "Q", "None");
        c.load(&json);
        assert_eq!(c.json(), json);
    }

    #[test]
    fn floats_print_the_way_json_net_does() {
        let t = [(1.0, "1.0"), (483.6, "483.6"), (0.25, "0.25"), (90.0, "90.0"), (-0.5, "-0.5"), (0.00001, "1E-05"), (1e15, "1E+15"), (0.0, "0.0")];
        for (v, want) in t {
            assert_eq!(float_text(v), want, "{v}");
        }
    }

    /// UserPreferenceManager.Save and InputSystem.Save against the game's own files: read one in and write it out, and the
    /// bytes are the same.
    #[test]
    fn saved_files_are_the_games_bytes() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        if let Ok(text) = std::fs::read_to_string(game.join("config.json")) {
            let mut s = Settings::default();
            s.load(&text);
            let screen = s.resolution.unwrap_or((1920, 1080));
            assert_eq!(s.save_text(screen), text, "config.json");
        }
        if let Ok(text) = std::fs::read_to_string(game.join("ControlScheme.json")) {
            let mut c = Controls::default();
            c.load(&text);
            assert_eq!(c.json(), text, "ControlScheme.json");
        }
    }

    /// Resolution: the window's size while windowed, kept as read otherwise, null when automatic.
    #[test]
    fn resolution_follows_the_window_mode() {
        let mut s = Settings::default();
        assert!(s.save_text((800, 600)).contains("\"Resolution\": null"));
        s.set(Settings::index("WindowMode"), 0.0);
        let text = s.save_text((800, 600));
        assert!(text.contains("\"Resolution\": {\r\n    \"x\": 800,\r\n    \"y\": 600\r\n  }"));
        s.set(Settings::index("WindowMode"), 1.0);
        assert!(s.save_text((1, 2)).contains("\"x\": 800"));
        assert!(text.starts_with("{\r\n  \"CollisionQuality\": \"Dynamic\",\r\n") && text.ends_with("\"ToyboxSizeOffset\": 483.6\r\n}"));
    }
}

/// The Stats page's tiles, Content's children in order (the same in Menu and Main).
pub const STAT_TILES: [&str; 6] = ["BODY_COUNT", "REVIVED", "TOTAL_SPAWNED_ITEMS", "NUCLEAR_EXPLOSIONS", "WIRES_LAID", "CONTRAPTIONS_SAVED"];

/// A Stats tile's scene path.
pub fn stat_tile_path(scene: &str, tile: &str) -> String {
    let page = if scene == "Main" { "Canvas/Pause screen/Stats" } else { "Canvas/Stats" };
    format!("{page}/body/Scroll View/Viewport/Content/{tile}")
}

/// A UIStatViewBehaviour field of a tile.
pub fn stat_view_field(scene: &str, tile: &str, name: &str) -> &'static crate::assets::mono::Value {
    super::scene::field(scene, &stat_tile_path(scene, tile), "UIStatViewBehaviour", name).unwrap_or_else(|| panic!("UIStatViewBehaviour.{name} of {tile}"))
}

fn stat_id(scene: &str, tile: &str) -> String {
    match stat_view_field(scene, tile, "StatID") {
        crate::assets::mono::Value::Str(s) => s.clone(),
        _ => panic!("UIStatViewBehaviour.StatID of {tile}"),
    }
}

/// UIStatViewBehaviour.Start: the icon and StatName are the view's; with GetFromSteam and SteamworksInitialiser.IsInitialised,
/// NonSteamStatManager.Stats.SetStat(StatID, StatManager.GetInt(StatID)); then UpdateValue (the caller's).
fn stat_view_start(scene: &str, tile: &str) {
    if stat_view_field(scene, tile, "GetFromSteam").bool() == Some(true) && crate::game::stats::STEAMWORKS_INITIALISED {
        let id = stat_id(scene, tile);
        let v = crate::game::stats::get_int(&id) as f32;
        crate::game::stats::with_stats(|s| s.set_stat(&id, v));
    }
}

/// UIStatViewBehaviour.UpdateValue: GetStat(StatID), Mathf.RoundToInt when RoundToInt, as float.ToString().
fn stat_view_value(scene: &str, tile: &str) -> String {
    let id = stat_id(scene, tile);
    let mut f = crate::game::stats::with_stats(|s| s.get_stat(&id, 0.0));
    if stat_view_field(scene, tile, "RoundToInt").bool() == Some(true) {
        // Mathf.RoundToInt: (int)Math.Round(f), ties to even.
        f = (f as f64).round_ties_even() as i32 as f32;
    }
    crate::game::menu::cs_float(f)
}
