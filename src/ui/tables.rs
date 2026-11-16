//! The game's own C# definitions (Preferences.cs, InputSystem.cs, EnvironmentalSettings.cs, ContextMenuBehaviour.cs)
//! and the menus' structure. Texts authored in the scenes and the ToolLibrary are read from the install at runtime
//! (`scene`, `art`), never copied here; so are the C#'s own texts and its InputSystem table (`assets::il`): the
//! attributes' titles, descriptions and formats, the enums' names, the default actions. The numbers stay here.

use std::sync::LazyLock;

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Bool,
    Enum(&'static [&'static str]),
    Int { min: f32, max: f32, step: f32 },
    Float { min: f32, max: f32, step: f32 },
    /// A float chosen from a fixed list (the Option attribute).
    Options(&'static [f32]),
}

/// The Format attribute: `template` with `{0}` replaced by `round(value * mult, digits)`.
#[derive(Clone, Copy, Debug)]
pub struct Fmt {
    pub template: &'static str,
    pub mult: f32,
    pub digits: i32,
}

impl Fmt {
    /// FormatAttribute..ctor: the Format, Multiplier and DigitCount.
    pub const fn new(template: &'static str, mult: f32, digits: i32) -> Self {
        Fmt { template, mult, digits }
    }

    /// FormatAttribute.FormatString (the float overload): string.Format(Format, Math.Round(value * Multiplier, DigitCount)).
    pub fn format_string(&self, value: f32) -> String {
        let x = (value * self.mult) as f64;
        let k = 10f64.powi(self.digits);
        self.template.replace("{0}", &format!("{}", (x * k).round_ties_even() / k))
    }

    /// FormatAttribute.FormatString (the int overload): string.Format(Format, (float)value * Multiplier).
    pub fn format_string_int(&self, value: i32) -> String {
        self.template.replace("{0}", &format!("{}", value as f32 * self.mult))
    }
}

/// One field of the game's Preferences class, in the settings menu's order.
#[derive(Clone, Copy, Debug)]
pub struct Setting {
    pub field: &'static str,
    /// Index into `CATEGORIES`; only meaningful when `menu`.
    pub category: usize,
    pub title: &'static str,
    pub description: &'static str,
    pub kind: Kind,
    pub default: f32,
    pub fmt: Option<Fmt>,
    /// Shown in the settings menu (has a Setting attribute and no HideInSettingsMenu).
    pub menu: bool,
}

/// SettingCategory.
/// Rewrite extra: "Mods" (after the game's), the extras' own settings (the game has no settings for mods).
pub static CATEGORIES: LazyLock<&'static [&'static str]> = LazyLock::new(|| {
    let mut c = crate::assets::il::get().enum_names("SettingCategory").to_vec();
    c.push("Mods");
    c.leak()
});

// Preferences..ctor: the field initialisers are each Setting's `default` here and Prefs::default's (game/mod.rs) for the fields the simulation reads
// StepAttribute..ctor: Step = step, a Kind's `step` (0 for a field with no Step attribute: `snap` leaves the value alone, as the null stepAttr does).
// MemberInfoExtensions.GetCustomAttribute: GetCustomAttributes(typeof(T), inherit: true).FirstOrDefault() as T. The attributes the game reads per member are
// compiled in: Setting, Step, Range, Option, Format, Sort and HideInSettingsMenu here; Obsolete, NonSerialized and SkipSerialisation in the save
// member table (game/save.rs stored_member); Range, TemperatureSetting and Tooltip in the environment settings controls (ui/view.rs).
// SettingAttribute..ctor: a Setting's category, title and description (the two-argument overload has an empty description)
/// Every Preferences field: `ROWS` with the C#'s texts (SettingAttribute's title and description, the rich-text tags taken
/// out; a field without one shows its name), FormatAttribute's Format and the enum's names.
pub static SETTINGS: LazyLock<Vec<Setting>> = LazyLock::new(|| {
    let t = crate::assets::il::get();
    ROWS.iter()
        .map(|&r| {
            let mut s = r;
            if t.field_attr("Preferences", s.field, "SettingAttribute").is_some() {
                s.title = t.field_attr_str("Preferences", s.field, "SettingAttribute", 1);
                s.description = leak(super::screens::untag(t.field_attr_str("Preferences", s.field, "SettingAttribute", 2)));
            } else {
                s.title = s.field;
            }
            if let Some(f) = &mut s.fmt {
                f.template = t.field_attr_str("Preferences", s.field, "FormatAttribute", 0);
            }
            if let Kind::Enum(_) = s.kind {
                s.kind = Kind::Enum(t.field_enum_names("Preferences", s.field));
            }
            s
        })
        .chain(EXTRAS.iter().copied())
        .collect()
});

/// Rewrite extras' settings (not Preferences fields), in the "Mods" category.
const EXTRAS: &[Setting] = &[Setting { field: "PortalSize", category: 6, title: "Portal size", description: "The length of the portal gun's openings, times the default. Applies to the next portal shot.", kind: Kind::Float { min: 0.5, max: 3.0, step: 0.05 }, default: 1.0, fmt: Some(Fmt::new("{0}x", 1.0, 2)), menu: true },
    Setting { field: "PortalGlow", category: 6, title: "Portal glow", description: "How brightly the portals and the portal gun glow, times the original.", kind: Kind::Float { min: 0.5, max: 3.0, step: 0.05 }, default: 1.5, fmt: Some(Fmt::new("{0}x", 1.0, 2)), menu: true },
    // Rewrite extras: what a portal shot into a person does (mishap_glue.rs); several on: one at random, none: it fizzles.
    Setting { field: "PortalMishapSuperposition", category: 6, title: "Portal mishap: superposition", description: "A portal shot into a person splits them into two flickering copies, here and at the other portal, that collapse after a few seconds.", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "PortalMishapFold", category: 6, title: "Portal mishap: spatial fold", description: "A portal shot into a person folds space around the limb it hits: it comes out of another limb through tiny portals, still attached.", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "PortalMishapPhase", category: 6, title: "Portal mishap: phase loss", description: "A portal shot into a person makes them drift through things until each limb snaps back into reality, crushing whatever it is inside.", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
];

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// The Preferences fields' numbers (texts filled in by `SETTINGS`).
const ROWS: &[Setting] = &[
    Setting { field: "GorelessMode", category: 1, title: "", description: "", kind: Kind::Bool, default: 0.0, fmt: None, menu: true },
    Setting { field: "ProceduralFragments", category: 1, title: "", description: "", kind: Kind::Bool, default: 0.0, fmt: None, menu: true },
    Setting { field: "CollisionQuality", category: 0, title: "", description: "", kind: Kind::Enum(&[]), default: 2.0, fmt: None, menu: true },
    Setting { field: "ToolSelectionMode", category: 0, title: "", description: "", kind: Kind::Enum(&[]), default: 0.0, fmt: None, menu: true },
    Setting { field: "MasterVolume", category: 5, title: "", description: "", kind: Kind::Float { min: 0.0, max: 1.0, step: 0.01 }, default: 1.0, fmt: Some(Fmt::new("", 100.0, 0)), menu: true },
    Setting { field: "WindowMode", category: 4, title: "", description: "", kind: Kind::Enum(&[]), default: 1.0, fmt: None, menu: true },
    Setting { field: "PhysicsIterations", category: 0, title: "", description: "", kind: Kind::Int { min: 8.0, max: 256.0, step: 0.0 }, default: 32.0, fmt: Some(Fmt::new("", 1.0, 0)), menu: true },
    Setting { field: "SfxVolume", category: 5, title: "", description: "", kind: Kind::Float { min: 0.0, max: 1.0, step: 0.01 }, default: 1.0, fmt: Some(Fmt::new("", 100.0, 0)), menu: true },
    Setting { field: "VSync", category: 4, title: "", description: "", kind: Kind::Bool, default: 0.0, fmt: None, menu: true },
    Setting { field: "RenderScale", category: 4, title: "", description: "", kind: Kind::Options(&[0.25, 0.5, 0.75, 1.0]), default: 1.0, fmt: Some(Fmt::new("", 100.0, 0)), menu: true },
    Setting { field: "AmbienceVolume", category: 5, title: "", description: "", kind: Kind::Float { min: 0.0, max: 1.0, step: 0.01 }, default: 1.0, fmt: Some(Fmt::new("", 100.0, 0)), menu: true },
    Setting { field: "UserInterfaceVolume", category: 5, title: "", description: "", kind: Kind::Float { min: 0.0, max: 1.0, step: 0.01 }, default: 1.0, fmt: Some(Fmt::new("", 100.0, 0)), menu: true },
    Setting { field: "AmbienceHighpassCutoff", category: 5, title: "", description: "", kind: Kind::Float { min: 50.0, max: 500.0, step: 1.0 }, default: 90.0, fmt: Some(Fmt::new("", 1.0, 0)), menu: true },
    Setting { field: "FramerateLimit", category: 4, title: "", description: "", kind: Kind::Int { min: 30.0, max: 1000.0, step: 0.0 }, default: 144.0, fmt: Some(Fmt::new("", 1.0, 0)), menu: true },
    Setting { field: "AmbientTemperatureTransfer", category: 0, title: "", description: "", kind: Kind::Bool, default: 0.0, fmt: None, menu: true },
    Setting { field: "Brightness", category: 4, title: "", description: "", kind: Kind::Float { min: 10.0, max: 400.0, step: 5.0 }, default: 100.0, fmt: Some(Fmt::new("", 1.0, 0)), menu: true },
    Setting { field: "TemperatureUnit", category: 3, title: "", description: "", kind: Kind::Enum(&[]), default: 0.0, fmt: None, menu: true },
    Setting { field: "TonemappingMode", category: 4, title: "", description: "", kind: Kind::Enum(&[]), default: 0.0, fmt: None, menu: true },
    Setting { field: "BloomMode", category: 2, title: "", description: "", kind: Kind::Enum(&[]), default: 2.0, fmt: None, menu: true },
    Setting { field: "StopAnimationOnDamage", category: 0, title: "", description: "", kind: Kind::Bool, default: 0.0, fmt: None, menu: true },
    Setting { field: "DropOnDeath", category: 0, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "ShakeIntensity", category: 0, title: "", description: "", kind: Kind::Float { min: 0.0, max: 2.0, step: 0.01 }, default: 1.0, fmt: Some(Fmt::new("", 1.0, 2)), menu: true },
    Setting { field: "ShowOutlines", category: 0, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "SlowMotionSpeed", category: 0, title: "", description: "", kind: Kind::Float { min: 1.0, max: 100.0, step: 1.0 }, default: 20.0, fmt: Some(Fmt::new("", 1.0, 0)), menu: true },
    Setting { field: "GridSnap", category: 0, title: "", description: "", kind: Kind::Float { min: 0.1, max: 2.0, step: 0.05 }, default: 0.25, fmt: Some(Fmt::new("", 1.0, 2)), menu: true },
    Setting { field: "AngleSnap", category: 0, title: "", description: "", kind: Kind::Int { min: 5.0, max: 90.0, step: 0.0 }, default: 15.0, fmt: Some(Fmt::new("", 1.0, 0)), menu: true },
    Setting { field: "ShowModLoadingFreeze", category: 0, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "FragilityMultiplier", category: 1, title: "", description: "", kind: Kind::Float { min: 0.05, max: 2.0, step: 0.05 }, default: 1.0, fmt: Some(Fmt::new("", 1.0, 3)), menu: true },
    Setting { field: "LimbCrushing", category: 1, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "BrainDamage", category: 1, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "GoreMode", category: 1, title: "", description: "", kind: Kind::Enum(&[]), default: 0.0, fmt: None, menu: true },
    Setting { field: "AutoHealWounds", category: 1, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "ChunkyShotParticles", category: 1, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "DismembermentLooseTissue", category: 1, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "CrushForceMultiplier", category: 1, title: "", description: "", kind: Kind::Float { min: 0.0, max: 2.0, step: 0.05 }, default: 1.0, fmt: Some(Fmt::new("", 100.0, 0)), menu: true },
    Setting { field: "TracerBullets", category: 2, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "FancyEffects", category: 2, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "Decals", category: 2, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "Lighting", category: 2, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "ClampVolume", category: 5, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "DistantSoundEffects", category: 5, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "BulletCrackWhiz", category: 5, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "LogDebugMessages", category: 3, title: "", description: "", kind: Kind::Bool, default: 0.0, fmt: None, menu: true },
    Setting { field: "ShowFramerate", category: 3, title: "", description: "", kind: Kind::Bool, default: 0.0, fmt: None, menu: true },
    Setting { field: "ZoomSensitivity", category: 3, title: "", description: "", kind: Kind::Float { min: 0.0, max: 2.0, step: 0.1 }, default: 1.0, fmt: Some(Fmt::new("", 1.0, 2)), menu: true },
    Setting { field: "ThermalVisionUpdateRate", category: 3, title: "", description: "", kind: Kind::Int { min: 10.0, max: 60.0, step: 5.0 }, default: 25.0, fmt: Some(Fmt::new("", 1.0, 0)), menu: true },
    Setting { field: "DeleteWireByContextMenu", category: 3, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: true },
    Setting { field: "SMAA", category: 4, title: "", description: "", kind: Kind::Bool, default: 0.0, fmt: None, menu: true },
    Setting { field: "SimplifiedBackground", category: 4, title: "", description: "", kind: Kind::Bool, default: 0.0, fmt: None, menu: true },
    Setting { field: "RejectShadyCode", category: 0, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: false },
    Setting { field: "MaxModCompilationTime", category: 0, title: "", description: "", kind: Kind::Int { min: 2.0, max: 120.0, step: 0.0 }, default: 30.0, fmt: Some(Fmt::new("", 1.0, 0)), menu: false },
    Setting { field: "LimbOverflowExplosion", category: 1, title: "", description: "", kind: Kind::Bool, default: 0.0, fmt: None, menu: false },
    Setting { field: "ShowTutorial", category: 0, title: "", description: "", kind: Kind::Bool, default: 1.0, fmt: None, menu: false },
    Setting { field: "ToyboxSizeOffset", category: 0, title: "", description: "", kind: Kind::Float { min: -1000000000.0, max: 1000000000.0, step: 0.0 }, default: 483.6, fmt: None, menu: false },
];

/// One InputSystem default action.
#[derive(Clone, Copy, Debug)]
pub struct ActionDef {
    pub id: &'static str,
    pub name: &'static str,
    pub key: &'static str,
    pub secondary: &'static str,
    pub invisible: bool,
    /// ActionCategory name
    pub category: &'static str,
    /// ActionUniverse name
    pub universe: &'static str,
}

/// InputSystem..cctor's Defaults, in order: each InputAction's id and Name (its two ldstr), then Key, SecondaryKey, Invisible,
/// Category and Universe (its five ldc.i4: KeyCode, bool and the two enums).
pub static ACTIONS: LazyLock<Vec<ActionDef>> = LazyLock::new(|| {
    let t = crate::assets::il::get();
    const M: &str = "InputSystem::.cctor";
    let key = |a: usize, j: usize| {
        let v = t.i4(M, 5 * a + j);
        crate::game::keycode::KEYCODES.iter().find(|k| k.0 == v).unwrap_or_else(|| panic!("no KeyCode {v}")).1
    };
    (0..t.count(M, "str") / 2)
        .map(|a| ActionDef {
            id: t.str(M, 2 * a),
            name: t.str(M, 2 * a + 1),
            key: key(a, 0),
            secondary: key(a, 1),
            invisible: t.i4(M, 5 * a + 2) != 0,
            category: t.enum_name("ActionCategory", t.i4(M, 5 * a + 3) as i64),
            universe: t.enum_name("ActionUniverse", t.i4(M, 5 * a + 4) as i64),
        })
        .collect()
});

/// ControlSchemeEditorBehaviour's section order (ActionCategory without MapEditor).
pub static ACTION_CATEGORIES: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| crate::assets::il::get().enum_names("ActionCategory").iter().copied().filter(|&c| c != "MapEditor").collect());

/// A button of a menu page: (its GameObject's name under the page's Menu, shown, target page, kind). `kind` is page / quit / scene / "".
/// The label and tooltip are the scene's own (`scene::label`, `scene::tooltip`).
pub type Entry = (&'static str, bool, &'static str, &'static str);
pub const MAIN_MENU: &[Entry] = &[
    ("Play", true, "Map Selection", "page"),
    ("Settings", true, "Settings", "page"),
    ("Player stats", false, "Stats", "page"),
    ("Texture packs", false, "Texture packs", "page"),
    ("Mods", true, "Mods", "page"),
    ("Map editor", false, "", "scene"),
    ("Information", true, "Information", "page"),
    ("Report bug", false, "Bug report form", "page"),
    ("Quit", true, "", "quit"),
];
/// The Menu scene's path of MAIN_MENU's parent.
pub const MAIN_MENU_PATH: &str = "Canvas/Main/Menu";

pub const PAUSE_MENU: &[Entry] = &[
    ("Resume", true, "", ""),
    ("Restart", true, "", "scene"),
    ("Choose Map", true, "Map Selection", "page"),
    ("Stats", true, "Stats", "page"),
    ("Options", true, "Settings", "page"),
    ("Bug report", false, "Bug report form", "page"),
    ("Quit", true, "", "scene"),
];
/// The Main scene's path of PAUSE_MENU's parent.
pub const PAUSE_MENU_PATH: &str = "Canvas/Pause screen/Main/Menu";

/// The settings page's left column in order: (the Menu scene's button under Canvas/Settings/Menu, kind); kind is a category name, `Controls` or `Back`.
pub const SETTINGS_MENU: &[(&str, &str)] = &[
    ("general settings", "General"),
    ("control settings", "Controls"),
    ("gore settings", "Gore"),
    ("visual effects settings", "VisualEffects"),
    ("ui settings", "UserInterface"),
    ("audio settings", "Audio"),
    ("video settings", "Video"),
    // Rewrite extra: not in the scene, drawn like the tabs above.
    ("mods settings", "Mods"),
    ("back", "Back"),
];

/// EnvironmentalSettings: (label, is bool, tooltip, range, default, is temperature). The label is the field's name (its `_` as
/// spaces), the tooltip its TooltipAttribute's, in the fields' declaration order; the numbers are `ENVIRONMENT_ROWS`.
pub static ENVIRONMENT: LazyLock<Vec<(&'static str, bool, &'static str, Option<(f32, f32)>, f32, bool)>> = LazyLock::new(|| {
    let t = crate::assets::il::get();
    let fields = t.fields("EnvironmentalSettings");
    assert_eq!(fields.len(), ENVIRONMENT_ROWS.len(), "EnvironmentalSettings' fields");
    fields
        .iter()
        .zip(ENVIRONMENT_ROWS)
        .map(|(&f, &(bool, range, default, temperature))| {
            (leak(f.replace('_', " ")), bool, t.field_attr_str("EnvironmentalSettings", f, "TooltipAttribute", 0), range, default, temperature)
        })
        .collect()
});

/// EnvironmentalSettings' fields in order: (is bool, range, default, is temperature).
const ENVIRONMENT_ROWS: &[(bool, Option<(f32, f32)>, f32, bool)] = &[
    (true, None, 1.0, false),
    (true, None, 0.0, false),
    (true, None, 0.0, false),
    (true, None, 0.0, false),
    (false, Some((-200.0, 200.0)), -9.81, false),
    (false, Some((0.0, 100.0)), 0.0, false),
    (false, Some((-100.0, 9000.0)), 18.0, true),
];

/// The context menu's static buttons: (GameObject name under Canvas/Scrolling context menu/Viewport/Content, ContextMenuBehaviour
/// condition property). The label is its `Text` child's, the tooltip its HasTooltipBehaviour's (`scene`).
pub const CONTEXT_MENU: &[(&str, &str)] = &[
    ("Delete", "CanDelete"),
    ("Copy", "HasObject"),
    ("Paste", "HasCopied"),
    ("Save", "HasObject"),
    ("-", ""),
    ("Follow", "HasObject"),
    ("Activate", "HasObject"),
    ("Ignite", "HasObject"),
    ("Freeze", "HasObject"),
    ("No Collide", "HasObject"),
    ("Gravity", "HasObject"),
    ("Resize", "ShouldShowResizeButton"),
    ("Edit layer", "ShouldShowLayerButton"),
    ("-", ""),
];
pub const CONTEXT_MENU_PATH: &str = "Canvas/Scrolling context menu/Viewport/Content";

#[cfg(test)]
mod format_tests {
    use super::Fmt;

    #[test]
    fn format_attribute_strings() {
        let percent = Fmt::new("{0}%", 100.0, 0);
        assert_eq!(percent.format_string(0.76), "76%");
        assert_eq!(percent.format_string_int(2), "200%");
        assert_eq!(Fmt::new("{0} iterations", 1.0, 0).format_string_int(32), "32 iterations");
        assert_eq!(Fmt::new("{0}s", 1.0, 2).format_string(0.12345), "0.12s");
    }
}
