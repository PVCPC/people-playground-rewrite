//! Spawns the UI's entities for the current state; `live` keeps their texts and colours current in between.
//! Every size is in the scenes' canvas units: the reference 2688 wide, scaled by UiScale.

use super::art::{Art, ToolDef, UiArt, FONT};
use super::scene;
use super::state::*;
use super::tables::*;
use bevy::{
    picking::hover::Hovered,
    prelude::*,
    text::FontSize,
    ui::{BackgroundGradient, ColorStop, InterpolationColorSpace, LinearGradient, RelativeCursorPosition, ScrollPosition},
};

const BACKDROP: Color = Color::srgb(0.129, 0.137, 0.153);
const BLACK: Color = Color::BLACK;
const WHITE: Color = Color::WHITE;
const HANDLE: &str = "Canvas/Catalog/resize handle";
/// ResizableUIElementHandle's SizeRange and Step.
pub fn handle_range() -> ([f32; 2], f32) {
    (scene::vec2("Main", HANDLE, "ResizableUIElementHandle", "SizeRange"), scene::number("Main", HANDLE, "ResizableUIElementHandle", "Step"))
}

/// ResizableUIElementHandle's Default, Hover and Active colours.
pub fn handle_color(name: &str) -> Color {
    scene::color_field("Main", HANDLE, "ResizableUIElementHandle", name)
}
const CLEAR: Color = Color::NONE;
const HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.1);
const SELECTED: Color = Color::srgba(1.0, 1.0, 1.0, 0.2);
const SELECTED_HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.28);
const CONFLICT: Color = Color::srgba(0.6, 0.1, 0.1, 0.6);
const DIM: Color = Color::srgba(1.0, 1.0, 1.0, 0.4);
const WARN: Color = Color::srgb(0.95, 0.55, 0.45);
/// SettingButtonBehaviour's checked label.
pub const CHECKED: Color = Color::srgb(0.0, 1.0, 0.0);

/// The "not rewritten yet" marker the UI uses wherever the rewrite has nothing behind a control.
pub const NOT_YET: &str = "not rewritten yet";

/// CatalogBehaviour.LoadToyboxElementSize: the catalog panel's width (the resize handle's ToResize.sizeDelta.x): what the handle has set, else ToyboxSizeOffset.
pub fn catalog_width(ui: &Ui, settings: &Settings) -> f32 {
    ui.toybox.unwrap_or_else(|| settings.get("ToyboxSizeOffset"))
}

/// The catalog's ResizableUIElementHandle: 14 wide at the panel's right edge, over its full height.
#[derive(Debug, Default, Component)]
pub struct ResizeHandle;

#[derive(Component)]
pub struct Root;

/// A widget a click activates.
#[derive(Component)]
pub struct Press(pub Act);

/// "title\ndescription", shown while the pointer is over it: under the catalog, or by the toolbar for a tool.
#[derive(Component)]
pub struct Tip(pub String, pub bool);

/// The black box a catalog tooltip shows in, and the one by the toolbar.
#[derive(Component)]
pub struct TipBox(pub bool);

/// ToolBarGenerator.CreateChildContainerFor: a tool's "child container" of variants, by the parent tool's name.
#[derive(Component)]
pub struct Flyout(pub String);

/// A variant's button in the child container of the tool named.
#[derive(Component)]
pub struct InFlyout(pub String);

/// The child containers' activeSelf and whether their parent button was hovered last frame, by parent tool name (kept
/// across view rebuilds, as the toolbar's objects are).
#[derive(Resource, Default)]
pub struct Flyouts(pub std::collections::HashMap<String, (bool, bool)>);

/// ToolWheelBehaviour.EyeImage (its colour is set every frame from the game's eye animation).
#[derive(Component)]
pub struct EyeImage;

/// The text an italic run's picture shows (`screens` reads it).
#[derive(Component)]
pub struct ShownText(pub String);

/// A row of a rich text line's runs: `screens` reads it as the one text it is.
#[derive(Component)]
pub struct RichLine;

/// What `screens` leaves out, and why: an outline copy of a text, a rewrite feature or "not rewritten yet" note the game
/// lacks, or what differs only because the game ran with Steam.
#[derive(Component)]
pub struct NotCompared(pub &'static str);

/// TextMesh Pro rich text as runs of (text, bold, colour), white by default: <b>, </b>, <color=#rrggbb[aa]>,
/// <color="name"> and </color> (a stack, as TMP's colour stack); any other tag is drawn as text.
pub fn rich_runs(text: &str) -> Vec<(String, bool, Color)> {
    const NAMES: [(&str, [f32; 3]); 8] = [("red", [1.0, 0.0, 0.0]), ("white", [1.0; 3]), ("black", [0.0; 3]), ("green", [0.0, 1.0, 0.0]), ("blue", [0.0, 0.0, 1.0]), ("orange", [1.0, 0.5019608, 0.0]), ("purple", [0.627451, 0.1254902, 0.9411765]), ("yellow", [1.0, 1.0, 0.0])];
    let (mut runs, mut cur, mut bold, mut colors) = (Vec::new(), String::new(), 0u32, vec![WHITE]);
    let mut rest = text;
    while let Some(i) = rest.find('<') {
        cur.push_str(&rest[..i]);
        let after = &rest[i..];
        let Some(j) = after.find('>') else { break };
        let tag = &after[1..j];
        let hex = |h: &str| -> Option<Color> {
            let v = |k: usize| u8::from_str_radix(h.get(k..k + 2)?, 16).ok().map(|x| x as f32 / 255.0);
            match h.len() {
                6 => Some(Color::srgb(v(0)?, v(2)?, v(4)?)),
                8 => Some(Color::srgba(v(0)?, v(2)?, v(4)?, v(6)?)),
                _ => None,
            }
        };
        let change = match tag {
            "b" => Some(Some(true)),
            "/b" => Some(Some(false)),
            "/color" => Some(None),
            _ => None,
        };
        let colour = tag.strip_prefix("color=").map(|v| v.trim_matches('"')).and_then(|v| match v.strip_prefix('#') {
            Some(h) => hex(h),
            None => NAMES.iter().find(|n| n.0 == v).map(|n| Color::srgb(n.1[0], n.1[1], n.1[2])),
        });
        if change.is_none() && colour.is_none() {
            cur.push('<');
            rest = &after[1..];
            continue;
        }
        if !cur.is_empty() {
            runs.push((std::mem::take(&mut cur), bold > 0, *colors.last().unwrap()));
        }
        match (change, colour) {
            (Some(Some(true)), _) => bold += 1,
            (Some(Some(false)), _) => bold = bold.saturating_sub(1),
            (Some(None), _) if colors.len() > 1 => _ = colors.pop(),
            (_, Some(c)) => colors.push(c),
            _ => {}
        }
        rest = &after[j + 1..];
    }
    cur.push_str(rest);
    if !cur.is_empty() {
        runs.push((cur, bold > 0, *colors.last().unwrap()));
    }
    runs
}

/// SelectionBox (Main scene, under the Canvas): top-left pivot, an Image (green at 0.26666668, the SelectionBox sprite
/// sliced, 2-texel borders) and its Width display (above, centred, bottom-aligned less 5) and Height display (left of
/// it, right-aligned less 5, middle): TextMeshPro 24 in the same colour, each with ExistInDetailView (active only in
/// detail view). SelectionBoxBehaviour.Start hides it.
#[derive(Component)]
pub struct SelBox;

/// The Width display (true) or Height display (false).
#[derive(Component)]
pub struct SelBoxText(pub bool);

const SELECTION_BOX: &str = "Canvas/SelectionBox";

fn selection_box(root: &mut Kids, ui: &Ui, art: &UiArt) {
    let mut e = root.spawn((Node { position_type: PositionType::Absolute, display: Display::None, ..default() }, SelBox, Pickable::IGNORE));
    if let Some(a) = art.named.get("SelectionBox") {
        let slicer = TextureSlicer { border: BorderRect::all(2.0), center_scale_mode: SliceScaleMode::Stretch, sides_scale_mode: SliceScaleMode::Stretch, max_corner_scale: 1.0 };
        e.insert(ImageNode { image: a.image.clone(), rect: Some(a.rect), color: scene::image_color("Main", SELECTION_BOX), image_mode: NodeImageMode::Sliced(slicer), ..default() });
    }
    if !ui.detail {
        return;
    }
    e.with_children(|b| {
        let text = scene::color_field("Main", &format!("{SELECTION_BOX}/Width display"), "TextMeshProUGUI", "m_fontColor");
        let (width_at, height_at) = (scene::rect("Main", &format!("{SELECTION_BOX}/Width display")), scene::rect("Main", &format!("{SELECTION_BOX}/Height display")));
        // Width display: anchors (0,1)-(1,1), pivot (0.5,1), y 39.44503, height 39.25; margin bottom 5.
        b.spawn((Node { position_type: PositionType::Absolute, left: px(0), right: px(0), top: px(-width_at.anchored_position[1]), height: px(width_at.size_delta[1]), justify_content: JustifyContent::Center, align_items: AlignItems::FlexEnd, padding: UiRect::bottom(px(5)), ..default() }, Pickable::IGNORE)).with_children(|w| {
            w.spawn((Text::new(""), font(24.0), TextColor(text), TextLayout::default().with_no_wrap(), SelBoxText(true), Pickable::IGNORE));
        });
        // Height display: anchors (0,0)-(0,1), pivot (0,0.5), x -86.19495, width 86.54999; margin right 5.
        b.spawn((Node { position_type: PositionType::Absolute, left: px(height_at.anchored_position[0]), width: px(height_at.size_delta[0]), top: px(0), bottom: px(0), justify_content: JustifyContent::FlexEnd, align_items: AlignItems::Center, padding: UiRect::right(px(5)), ..default() }, Pickable::IGNORE)).with_children(|h| {
            h.spawn((Text::new(""), font(24.0), TextColor(text), TextLayout::default().with_no_wrap(), SelBoxText(false), Pickable::IGNORE));
        });
    });
}

/// The NotificationArea.
#[derive(Component)]
pub struct NoteArea;

/// One Notification instance: the real time it was made (its key in Ui.notes).
#[derive(Component)]
pub struct Note(pub f64);

/// The Notification's TextMeshPro Text, which NotificationAnimationBehaviour moves.
#[derive(Component)]
pub struct NoteText;

/// Notification.prefab: a 337.7 by 36 box (its Image disabled) holding the TextMeshPro Text: fills it less the margins
/// (0.48, 0.48, 5, 0), 18, UpperCase, character spacing -3.39, right and middle aligned, no wrapping, overflow; the
/// legacy Text child is inactive.
pub fn note(p: &mut Kids, text: &str, born: f64) {
    const NOTE: &str = "Canvas/NotificationArea/Notification";
    let note_box = scene::rect("Main", NOTE);
    let margin = scene::field("Main", &format!("{NOTE}/TextMeshPro Text"), "TextMeshProUGUI", "m_margin").unwrap_or_else(|| panic!("no note margin"));
    let margin = ["x", "y"].map(|k| margin.get(k).and_then(crate::assets::mono::Value::f32).unwrap_or(0.0));
    let spacing = scene::number("Main", &format!("{NOTE}/TextMeshPro Text"), "TextMeshProUGUI", "m_characterSpacing");
    p.spawn((Node { width: px(note_box.size_delta[0]), height: px(note_box.size_delta[1]), flex_shrink: 0.0, ..default() }, Note(born), Pickable::IGNORE)).with_children(|n| {
        n.spawn((Node { position_type: PositionType::Absolute, left: px(margin[0]), right: px(5), top: px(margin[1]), bottom: px(0), justify_content: JustifyContent::FlexEnd, align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|t| {
            t.spawn((Node { flex_shrink: 0.0, ..default() }, NoteText, Pickable::IGNORE)).with_children(|row| {
                for (s, strong, color) in rich_runs(text) {
                    let mut e = row.spawn((Text::new(s.to_uppercase()), font(18.0), TextColor(color), bevy::text::LetterSpacing::Px(spacing * 18.0 / 100.0), TextLayout::default().with_no_wrap(), Pickable::IGNORE));
                    if strong {
                        e.insert(TextShadow { offset: Vec2::new(1.2, 0.0), color });
                    }
                }
            });
        });
    });
}

/// A Status bar item StatusController.Update turns on and off: TimeScaleView, FramerateView.
#[derive(Component, Clone, Copy, PartialEq)]
pub enum StatusItem {
    TimeScale,
    Framerate,
}

#[derive(Component)]
pub enum Bound {
    Setting(usize),
    Bind(usize),
    /// TimeScaleViewBehaviour's Text.
    TimeScale,
    /// FramerateViewBehaviour's Text, the number part (the SmallCaps "FPS" is a fixed span).
    Framerate,
    Tip(bool, bool),
    Filter,
    /// The environment window's Tooltip text (EnvironmentSettingsController.TooltipTarget).
    EnvTip,
}

/// A HasTooltipBehaviour whose TooltipText is the environment window's Tooltip text: its Text.
#[derive(Component)]
pub struct EnvTip(pub String);

#[derive(Component)]
pub struct SliderOf(pub usize);

/// PianoKey: the sharps are the black keys.
const PIANO_BLACK: [bool; 12] = [false, true, false, true, false, false, true, false, true, false, true, false];

/// The white keys left of black key k: its white key in the same octave plus every whole octave before it.
fn white_before(keys: &[(String, usize)], k: usize) -> usize {
    let octave = keys[k].0.rsplit(' ').next().and_then(|o| o.parse::<usize>().ok()).unwrap_or(3) - 3;
    octave * 7 + PIANO_BLACK[..keys[k].1].iter().filter(|b| !**b).count()
}

/// The colour picker's saturation/value box and hue bar.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub enum ColourArea {
    SaturationValue,
    Hue,
}

#[derive(Component)]
pub struct FillOf(pub usize);

#[derive(Component)]
pub struct Scroll(pub u8, pub f32);

/// What makes a widget look selected.
#[derive(Clone, PartialEq)]
pub enum Sel {
    None,
    Setting(usize, f32),
    Tab(usize),
    Powers(bool),
    Env,
    Map(usize),
    Bind(usize),
    Tool(String),
}

impl Sel {
    pub fn selected(&self, ui: &Ui, settings: &Settings, _: &Controls) -> bool {
        match self {
            Sel::None => false,
            Sel::Setting(i, v) => settings.values[*i] == *v,
            Sel::Tab(t) => ui.tab == *t,
            Sel::Powers(p) => ui.powers == *p,
            Sel::Env => ui.env_open,
            Sel::Map(i) => ui.selected == Some(*i),
            Sel::Bind(i) => ui.rebinding == Some(*i),
            Sel::Tool(t) => ui.tool == *t,
        }
    }
}

#[derive(Component)]
pub struct Look {
    pub sel: Sel,
    pub on: bool,
}

impl Look {
    pub fn colour(&self, selected: bool, hovered: bool, _: &Ui, controls: &Controls) -> Color {
        if !self.on {
            return CLEAR;
        }
        let conflict = matches!(self.sel, Sel::Bind(i) if controls.conflicts(i));
        match (selected, hovered, conflict) {
            (true, true, _) => SELECTED_HOVER,
            (true, false, _) => SELECTED,
            (false, true, _) => HOVER,
            (false, false, true) => CONFLICT,
            _ => CLEAR,
        }
    }
}

/// A label that turns green while its choice is the setting's value.
#[derive(Component)]
pub struct Checked(pub Sel);

type Kids<'a> = ChildSpawnerCommands<'a>;

fn font(size: f32) -> TextFont {
    let mut f = TextFont { font_size: FontSize::Px(size), ..default() };
    if let Some(h) = FONT.get() {
        f.font = h.clone().into();
    }
    f
}

fn label(p: &mut Kids, s: impl Into<String>, size: f32, color: Color) {
    p.spawn((Text::new(s), font(size), TextColor(color), Pickable::IGNORE));
}

/// TextMesh Pro's <b>: the game's fonts have no bold face, it thickens the glyphs.
fn bold(p: &mut Kids, s: impl Into<String>, size: f32, color: Color) {
    p.spawn((Text::new(s), font(size), TextColor(color), TextShadow { offset: Vec2::new(1.2, 0.0), color }, Pickable::IGNORE));
}

fn bound(p: &mut Kids, b: Bound, size: f32, color: Color) {
    p.spawn((Text::new(""), font(size), TextColor(color), b, Pickable::IGNORE));
}

/// A sprite letterboxed into a w by h box whose top left is the parent's.
fn picture(p: &mut Kids, a: &Art, w: f32, h: f32, tint: Color) {
    picture_with(p, a, w, h, tint, ());
}

fn picture_with(p: &mut Kids, a: &Art, w: f32, h: f32, tint: Color, extra: impl Bundle) {
    let pw = w.min(h * a.aspect);
    let ph = pw / a.aspect;
    p.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px((w - pw) / 2.0 + a.inner[0] * pw),
            top: px((h - ph) / 2.0 + a.inner[1] * ph),
            width: px(a.inner[2] * pw),
            height: px(a.inner[3] * ph),
            ..default()
        },
        ImageNode { image: a.image.clone(), rect: Some(a.rect), color: tint, ..default() },
        Pickable::IGNORE,
        extra,
    ));
}

/// Plain text on nothing, the way every menu button is; without an act it is disabled.
fn text_button(p: &mut Kids, text: &str, size: f32, act: Option<Act>, sel: Sel, tip: &str, node: Node) {
    text_button_with(p, text, size, act, sel, tip, node, ());
}

fn text_button_with(p: &mut Kids, text: &str, size: f32, act: Option<Act>, sel: Sel, tip: &str, node: Node, extra: impl Bundle) {
    let on = act.is_some();
    let mut e = p.spawn((node, BackgroundColor(CLEAR), Hovered::default(), Look { sel, on }, Tip(tip.into(), false), extra));
    if let Some(a) = act {
        e.insert(Press(a));
    }
    e.with_children(|c| label(c, text, size, if on { WHITE } else { DIM }));
}

fn menu_node() -> Node {
    Node { width: px(200), height: px(50), flex_shrink: 0.0, align_items: AlignItems::Center, ..default() }
}

fn wide_node() -> Node {
    Node { width: percent(100), height: px(30), flex_shrink: 0.0, padding: UiRect::horizontal(px(16)), align_items: AlignItems::Center, ..default() }
}

fn column(gap: f32) -> Node {
    Node { flex_direction: FlexDirection::Column, row_gap: px(gap), ..default() }
}

fn row(gap: f32) -> Node {
    Node { flex_direction: FlexDirection::Row, column_gap: px(gap), align_items: AlignItems::Center, ..default() }
}

fn scroll_node() -> Node {
    Node { flex_grow: 1.0, min_height: px(0), flex_direction: FlexDirection::Column, overflow: Overflow::scroll_y(), ..default() }
}

/// Scroll view `id`'s ScrollRect m_ScrollSensitivity (its OnScroll moves the content by the wheel delta times it). The local
/// achievements page (9) is laid out on the licenses' container and scrolls as it does.
fn scroll_sensitivity(ui: &Ui, id: u8) -> f32 {
    let (scene_name, prefix) = if ui.mode == Mode::Pause { ("Main", "Canvas/Pause screen/") } else { ("Menu", "Canvas/") };
    let (scene_name, path) = match id {
        0 => (scene_name, format!("{prefix}Map Selection/Map menu/Scroll View")),
        1 => (scene_name, format!("{prefix}Settings/Settings menu/NEW SCROLL VIEW")),
        2 => (scene_name, format!("{prefix}Controls/Control scheme editor/Scroll View")),
        3 => ("Main", "Canvas/Catalog/ItemPanel".into()),
        4 | 9 => ("Menu", "Canvas/Open-source licenses/info container".into()),
        5 => (scene_name, format!("{prefix}Mods/Panel/mod list")),
        6 => (scene_name, format!("{prefix}Mods/Panel/tools/mod description container")),
        11 => ("Main", "Canvas/Catalog/Environment options/options".into()),
        // Its content moves in canvas units drawn at ContextMenuBehaviour.Update's localScale.
        CONTEXT_SCROLL => return scene::field("Main", CONTEXT_MENU_SCENE, "ScrollRect", "m_ScrollSensitivity").and_then(crate::assets::mono::Value::f32).unwrap_or(1.0) * CONTEXT_SCALE,
        10 => (scene_name, format!("{prefix}Stats/body/Scroll View")),
        _ => unreachable!("scroll view {id}"),
    };
    // Headless tests have no scenes loaded.
    scene::field(scene_name, &path, "ScrollRect", "m_ScrollSensitivity").and_then(crate::assets::mono::Value::f32).unwrap_or(1.0)
}

/// A scroll area's Scrollbar in the scenes: (scene, ScrollRect path, Scrollbar path). The pause screen's copies live in Main under "Pause screen".
fn scrollbar_of(ui: &Ui, id: u8) -> Option<(&'static str, String, String)> {
    let (scene_name, prefix) = if ui.mode == Mode::Pause { ("Main", "Canvas/Pause screen/") } else { ("Menu", "Canvas/") };
    let (rect, bar) = match id {
        0 => ("Map Selection/Map menu/Scroll View", "Map Selection/Map menu/Scroll View/Scrollbar Vertical"),
        1 => ("Settings/Settings menu/NEW SCROLL VIEW", "Settings/Settings menu/NEW SCROLL VIEW/Scrollbar Vertical"),
        2 => ("Controls/Control scheme editor/Scroll View", "Controls/Control scheme editor/Scroll View/Scrollbar Vertical"),
        3 => return Some(("Main", "Canvas/Catalog/ItemPanel".into(), "Canvas/Catalog/ItemPanel/Viewport/Scrollbar".into())),
        CONTEXT_SCROLL => return Some(("Main", CONTEXT_MENU_SCENE.into(), format!("{CONTEXT_MENU_SCENE}/Scrollbar Vertical"))),
        4 => ("Open-source licenses/info container", "Open-source licenses/info container/Scrollbar"),
        5 => ("Mods/Panel/mod list", "Mods/Panel/mod list/Scrollbar"),
        6 => ("Mods/Panel/tools/mod description container", "Mods/Panel/tools/mod description container/Scrollbar"),
        _ => return None,
    };
    let (rect, bar) = (format!("{prefix}{rect}"), format!("{prefix}{bar}"));
    // The Settings and Controls pages of Menu are "Settings" and "Controls" objects; a page the scene lacks draws no bar.
    scene::has(scene_name, &bar).then_some((scene_name, rect, bar))
}

/// A ScrollRect: the viewport (scrolling node) with its Scrollbar over it. The viewport fills a container that takes `node`'s place in the layout.
fn scroll_area(p: &mut Kids, ui: &Ui, id: u8, node: Node, fill: impl FnOnce(&mut Kids)) {
    let container = Node { flex_grow: node.flex_grow, min_height: node.min_height, ..default() };
    let viewport = Node { position_type: PositionType::Absolute, left: px(0), right: px(0), top: px(0), bottom: px(0), flex_grow: 0.0, min_height: Val::Auto, ..node };
    p.spawn(container).with_children(|c| {
        c.spawn((viewport, ScrollPosition(Vec2::new(0.0, ui.scroll.get(&id).copied().unwrap_or(0.0))), Scroll(id, scroll_sensitivity(ui, id)), Hovered::default())).with_children(fill);
        if let Some((scene_name, rect, bar)) = scrollbar_of(ui, id) {
            super::scrollbar::spawn(c, id, scene_name, &rect, &bar, if id == CONTEXT_SCROLL { CONTEXT_SCALE } else { 1.0 });
        }
    });
}

/// ControlSchemeEditorBehaviour: the category name with a space before each capital.
fn spaced(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && c.is_uppercase() {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

fn category_index(key: &str) -> usize {
    CATEGORIES.iter().position(|c| *c == key).expect("a SettingCategory")
}

pub fn build(commands: &mut Commands, ui: &Ui, settings: &Settings, controls: &Controls, art: &UiArt, stopped: Option<&str>) {
    commands
        .spawn((
            Root,
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
            Pickable::IGNORE,
            GlobalZIndex(10),
        ))
        .with_children(|root| match ui.mode {
            Mode::Game => {
                if !ui.hide_ui {
                    hud(root, ui, settings, controls, art, stopped);
                }
            }
            Mode::Loading => loading_page(root, ui.loading_in_main, ui.load_progress),
            // The Menu scene's Canvas has no backdrop: its Panels sprite (ui::visual::menu_panels) shows through.
            Mode::Menu => screen(root, Color::NONE, |s| {
                page(s, ui, settings, controls, art);
                if let Some((title, state, workshop)) = &ui.loading_status {
                    loading_status(s, title, state, *workshop);
                }
            }),
            Mode::Pause => {
                hud(root, ui, settings, controls, art, stopped);
                screen(root, Color::srgba(0.09, 0.09, 0.09, 0.97), |s| page(s, ui, settings, controls, art));
            }
        });
}

/// The Menu scene's "background loading item status" (after the pages, so over them): a 500 by 128 Image (black at 0.9)
/// on the canvas's bottom centre holding the thumbnail (Unlit_UIFramesheet), the title and state TextMeshPro texts (30,
/// left aligned, bottom and top, no wrapping, Ellipsis overflow, their margins) and the "mask" Image over all (sliced,
/// Unlit_LoadingShader; its Mask is disabled). The "loading" child (RotateUI) is inactive.
fn loading_status(p: &mut Kids, title: &str, state: &str, workshop: bool) {
    const STATUS: &str = "Canvas/background loading item status";
    let size = scene::rect("Menu", STATUS).size_delta;
    let at = |path: &str| {
        let [left, top, width, height] = scene::placed("Menu", &format!("{STATUS}/{path}"), size);
        Node { position_type: PositionType::Absolute, left: px(left), top: px(top), width: px(width), height: px(height), ..default() }
    };
    let srgba = |path: &str| scene::image_color("Menu", &format!("{STATUS}/{path}")).to_srgba().to_f32_array();
    let text = |b: &mut Kids, path: &str, s: &str| {
        let full = format!("{STATUS}/{path}");
        let m = scene::field("Menu", &full, "TextMeshProUGUI", "m_margin").unwrap_or_else(|| panic!("no margin at {full}"));
        let [l, t, r, bm] = ["x", "y", "z", "w"].map(|k| m.get(k).and_then(crate::assets::mono::Value::f32).unwrap_or(0.0));
        // m_VerticalAlignment: 1024 Bottom, 256 Top.
        let bottom = scene::number("Menu", &full, "TextMeshProUGUI", "m_VerticalAlignment") == 1024.0;
        let justify = if bottom { JustifyContent::FlexEnd } else { JustifyContent::FlexStart };
        let size = scene::font_size("Menu", &full);
        b.spawn((Node { flex_direction: FlexDirection::Column, justify_content: justify, padding: UiRect { left: px(l), top: px(t), right: px(r), bottom: px(bm) }, overflow: Overflow::clip(), ..at(path) }, Pickable::IGNORE))
            .with_children(|c| rich(c, s, size, Justify::Left));
    };
    p.spawn((
        Node { position_type: PositionType::Absolute, left: percent(50), bottom: px(0), margin: UiRect::left(px(-size[0] / 2.0)), width: px(size[0]), height: px(size[1]), ..default() },
        BackgroundColor(scene::image_color("Menu", STATUS)),
        Pickable::IGNORE,
    ))
    .with_children(|b| {
        b.spawn((at("thumbnail"), super::loading::LoadingImage::Thumbnail { color: srgba("thumbnail"), workshop }, Pickable::IGNORE));
        text(b, "title", title);
        text(b, "state", state);
        b.spawn((at("mask"), super::loading::LoadingImage::Border { color: srgba("mask"), size }, Pickable::IGNORE));
    });
}

/// An absolute node for a RectTransform stretched over its parent (anchors 0 to 1): its offsets as insets.
fn stretched(r: &crate::assets::classes::RectTransform) -> Node {
    let min = |i: usize| r.anchored_position[i] - r.size_delta[i] * r.pivot[i];
    let max = |i: usize| r.anchored_position[i] + r.size_delta[i] * (1.0 - r.pivot[i]);
    Node { position_type: PositionType::Absolute, left: px(min(0)), right: px(-max(0)), bottom: px(min(1)), top: px(-max(1)), ..default() }
}

fn absolute() -> Node {
    Node { position_type: PositionType::Absolute, left: px(0), right: px(0), top: px(0), bottom: px(0), ..default() }
}


/// DialogBox.Start: the title, the input or key field only when the box has one, and the button holder; each DialogButton closes the box when ClosesDialogBox.
/// DialogBoxManager's "Dialog box" prefab: black, a VerticalLayoutGroup (padding 10, spacing 5), an Outline (white at
/// 0.282, 2 px) and a ContentSizeFitter over the 600 x 80 button holder.
/// The root's ContentSizeFitter (horizontal and vertical fit PreferredSize) sets its sizeDelta at every layout, so a
/// DialogBox.SetWidth or SetHeight (`DialogView::width`, `height`) does not show: the box is its content's size.
fn dialog_box(root: &mut Kids, content: impl FnOnce(&mut Kids)) {
    root.spawn(fill_centered()).with_children(|c| {
        let node = Node { min_width: px(620), min_height: px(200), padding: UiRect::all(px(scene::padding("Main", DIALOG_BOX, "VerticalLayoutGroup")[0])), align_items: AlignItems::Stretch, ..column(scene::spacing("Main", DIALOG_BOX, "VerticalLayoutGroup")) };
        c.spawn((node, BackgroundColor(BLACK), dialog_outline())).with_children(content);
    });
}

/// The dialog box the DialogBox prefab layout is read from.
/// DialogBoxManager's object (Main scene): the boxes are made under it.
pub const DIALOG_CONTAINER: &str = "Canvas/Dialog container";
const DIALOG_BOX: &str = "Canvas/Dialog container/Bell dialog box";

/// The dialogs' Outline (Bell dialog box): effect colour and distance.
fn dialog_outline() -> Outline {
    const BOX: &str = DIALOG_BOX;
    Outline { width: Val::Px(scene::vec2("Main", BOX, "Outline", "m_EffectDistance")[0]), offset: Val::Px(0.0), color: scene::color_field("Main", BOX, "Outline", "m_EffectColor") }
}

/// The dialog's title (centred).
fn dialog_title(b: &mut Kids, s: impl Into<String>, size: f32) {
    b.spawn((Text::new(s), font(size), TextColor(WHITE), TextLayout { justify: Justify::Center, ..default() }, Pickable::IGNORE));
}

/// The button holder (HorizontalLayoutGroup: padding 15, spacing 15) with DialogButton prefabs: Image 0.2264 tinted by
/// the normal colour 0.2642, bold centred text.
/// The Bell dialog box's Apply and Cancel buttons' titles.
fn apply() -> String {
    scene::label("Main", "Canvas/Dialog container/Bell dialog box/Button holder/Apply/Title")
}

fn cancel() -> String {
    scene::label("Main", "Canvas/Dialog container/Bell dialog box/Button holder/Cancel/Title")
}

fn dialog_buttons<const N: usize>(b: &mut Kids, node: Node, size: f32, buttons: [(&str, Act); N]) {
    b.spawn(Node { padding: UiRect::all(px(15)), column_gap: px(15), ..node }).with_children(|r| {
        for (text, act) in buttons {
            r.spawn((Node { flex_grow: 1.0, flex_basis: px(0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, dialog_button().bundle(), Hovered::default(), Press(act))).with_children(|t| tmp_bold(t, text, size, WHITE));
        }
    });
}

/// TMP FontStyles.SmallCaps (with Bold): lowercase letters drawn as capitals at 0.8 of the size.
fn small_caps(p: &mut Kids, s: &str, size: f32, color: Color, line: f32) {
    p.spawn((Text::default(), font(size), bevy::text::LineHeight::Px(line), TextColor(color), TextShadow { offset: Vec2::new(0.6, 0.0), color }, Pickable::IGNORE)).with_children(|t| {
        let mut runs: Vec<(bool, String)> = Vec::new();
        for ch in s.chars() {
            let lower = ch.is_lowercase();
            match runs.last_mut() {
                Some((l, run)) if *l == lower => run.extend(ch.to_uppercase()),
                _ => runs.push((lower, ch.to_uppercase().collect())),
            }
        }
        for (lower, run) in runs {
            t.spawn((TextSpan::new(run), font(if lower { size * 0.8 } else { size }), bevy::text::LineHeight::Px(line), TextColor(color)));
        }
    });
}

/// TMP bold with the Arial SDF assets (boldStyle 0.75): a slight thickening, lighter than `bold`.
fn tmp_bold(p: &mut Kids, s: impl Into<String>, size: f32, color: Color) {
    p.spawn((Text::new(s), font(size), TextColor(color), TextShadow { offset: Vec2::new(0.6, 0.0), color }, Pickable::IGNORE));
}

/// A piano key's label: GetDisplayName's lines, Arial letters with the sharp and flat signs from TMP's Bravura SDF
/// fallback (point size 154; glyph rects, sizes and bearings from its glyph table, the atlas 128 x 128).
fn piano_label(p: &mut Kids, s: &str, size: f32, ink: Color, bravura: Option<&Handle<Image>>) {
    let k = size / 154.0;
    p.spawn((Node { flex_direction: FlexDirection::Column, align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|c| {
        for line in s.split('\n') {
            c.spawn((Node { flex_direction: FlexDirection::Row, height: px(size * 1.15), ..default() }, Pickable::IGNORE)).with_children(|r| {
                for ch in line.chars() {
                    // (atlas rect x, y from the top, w, h; bearing y)
                    let glyph = match ch {
                        '\u{266d}' => Some((68.0, 128.0 - 10.0 - 95.0, 35.0, 95.0, 67.609375)),
                        '\u{266f}' => Some((10.0, 128.0 - 10.0 - 108.0, 39.0, 108.0, 53.90625)),
                        _ => None,
                    };
                    match (glyph, bravura) {
                        (Some((x, y, w, h, bearing)), Some(img)) => {
                            r.spawn((Node { width: px(w * k), height: px(h * k), margin: UiRect::top(px(size * 0.905 - bearing * k)), ..default() }, ImageNode { image: img.clone(), color: ink, rect: Some(Rect::new(x, y, x + w, y + h)), ..default() }, Pickable::IGNORE));
                        }
                        _ => {
                            r.spawn((Text::new(ch.to_string()), font(size), TextColor(ink), TextShadow { offset: Vec2::new(0.6, 0.0), color: ink }, Pickable::IGNORE));
                        }
                    }
                }
            });
        }
    });
}

/// Selectable's ColorTint transition: the Image's colour times the state's tint (normal, highlighted, pressed), faded in
/// over fadeDuration (Graphic.CrossFadeColor, real time). `down`: the press began on this object; `cur` the tint shown.
#[derive(Component, Clone, Copy)]
pub struct Tint {
    pub image: [f32; 4],
    pub normal: [f32; 4],
    pub high: [f32; 4],
    pub pressed: [f32; 4],
    /// m_SelectedColor, for the Buttons whose differs from Normal: shown while the EventSystem has the button selected
    /// (`ui::Selection`). The others keep None.
    pub selected: Option<[f32; 4]>,
    pub fade: f32,
    pub down: bool,
    pub cur: [f32; 4],
    /// The running ColorTween: its start, target and time so far.
    pub from: [f32; 4],
    pub to: [f32; 4],
    pub elapsed: f32,
}

impl Tint {
    /// A Button with an Image in the game's scenes: the image colour and the Button's Normal, Highlighted and Pressed colours and fade.
    pub fn of(scene: &str, path: &str) -> Tint {
        Tint::of_class(scene, path, "Button")
    }

    /// The same for any Selectable class (a TMP_InputField).
    pub fn of_class(scene: &str, path: &str, class: &str) -> Tint {
        Tint::of_graphic(scene, path, class, path)
    }

    /// The same with the Image on another object (the input field's is on its Text Area).
    pub fn of_graphic(scene: &str, path: &str, class: &str, image: &str) -> Tint {
        let c = scene::image_color(scene, image).to_srgba();
        let b = |n| scene::transition_color(scene, path, class, n);
        Tint::new([c.red, c.green, c.blue, c.alpha], b("m_NormalColor"), b("m_HighlightedColor"), b("m_PressedColor"), scene::transition_fade(scene, path, class))
    }

    pub const fn new(image: [f32; 4], normal: [f32; 4], high: [f32; 4], pressed: [f32; 4], fade: f32) -> Tint {
        Tint { image, normal, high, pressed, selected: None, fade, down: false, cur: normal, from: normal, to: normal, elapsed: 0.0 }
    }

    /// The Button's m_SelectedColor as well.
    pub fn with_selected(mut self, scene: &str, path: &str) -> Tint {
        self.selected = Some(scene::transition_color(scene, path, "Button", "m_SelectedColor"));
        self
    }

    pub fn color(&self) -> Color {
        let (i, c) = (self.image, self.cur);
        Color::srgba(i[0] * c[0], i[1] * c[1], i[2] * c[2], i[3] * c[3])
    }

    fn bundle(self) -> (Tint, BackgroundColor) {
        (self, BackgroundColor(self.color()))
    }
}

/// A Button whose target graphic is its picture (the Image child `picture` spawns): the tint goes to that ImageNode, not the background.
#[derive(Component)]
pub struct TintPicture;

/// DialogButton.prefab (read from the Bell dialog box's Apply): the Image and the Button tints.
fn dialog_button() -> Tint {
    Tint::of("Main", "Canvas/Dialog container/Bell dialog box/Button holder/Apply")
}

fn fill_centered() -> Node {
    Node { flex_direction: FlexDirection::Column, justify_content: JustifyContent::Center, align_items: AlignItems::Center, row_gap: px(24), ..absolute() }
}

/// The "Loading" page (the same in the Menu scene and under the Main scene's Pause screen, whose Image is its backdrop):
/// Logo(Text) (TMP lowercase, 70.7 at most) over the canvas, and the LoadingBarBehaviour bar below the centre. The scene
/// load's progress is `progress` (mod.rs `loading`).
/// LoadingBarBehaviour.SetProgress: background.localScale.x = progress, text = CeilToInt(progress * 100) + "%".
fn loading_page(root: &mut Kids, in_main: bool, progress: f32) {
    let (scene, at) = if in_main { ("Main", "Canvas/Pause screen/Loading") } else { ("Menu", "Canvas/Loading") };
    let back = if in_main { scene::image_color("Main", "Canvas/Pause screen") } else { BACKDROP };
    let bar = scene::rect(scene, &format!("{at}/Loading"));
    let fill = scene::rect(scene, &format!("{at}/Loading/ProgressBar"));
    let text = format!("{at}/Loading/TextMeshPro Text");
    let [w, h] = bar.size_delta;
    screen(root, back, |s| {
        s.spawn(fill_centered()).with_children(|c| label(c, scene::text(scene, &format!("{at}/Logo(Text)")).to_lowercase(), scene::font_size(scene, &format!("{at}/Logo(Text)")), WHITE));
        s.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(100.0 * bar.anchor_min[0]),
                top: Val::Percent(100.0 * (1.0 - bar.anchor_min[1])),
                margin: UiRect { left: px(bar.anchored_position[0] - w * bar.pivot[0]), top: px(-bar.anchored_position[1] - h * (1.0 - bar.pivot[1])), ..default() },
                width: px(w),
                height: px(h),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(scene::image_color(scene, &format!("{at}/Loading"))),
            Pickable::IGNORE,
        ))
        .with_children(|b| {
            b.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(fill.anchored_position[0]),
                    top: px(h * (1.0 - fill.anchor_min[1]) - fill.anchored_position[1] - fill.size_delta[1]),
                    width: px(fill.size_delta[0] * progress),
                    height: px(fill.size_delta[1]),
                    ..default()
                },
                BackgroundColor(scene::image_color(scene, &format!("{at}/Loading/ProgressBar"))),
                Pickable::IGNORE,
            ));
            label(b, format!("{}%", (progress * 100.0).ceil() as i32), scene::font_size(scene, &text), WHITE);
        });
    });
}

fn screen(root: &mut Kids, color: Color, content: impl FnOnce(&mut Kids)) {
    root.spawn((absolute(), BackgroundColor(color))).with_children(content);
}

/// The Menu column every page has at its left, vertically centred.
/// A page's Menu group (Menu scene `path`): full height at the left, its width and scale (pivot at the left edge), padding
/// 15 left, the 200 wide buttons middle centred.
fn side(p: &mut Kids, path: &str, content: impl FnOnce(&mut Kids)) {
    let r = scene::rect("Menu", path);
    let (w, s) = (r.size_delta[0], r.scale[0]);
    let pad = 15.0 + (w - 15.0 - 200.0) / 2.0;
    p.spawn((
        Node { position_type: PositionType::Absolute, left: px(0), top: px(0), bottom: px(0), width: px(w), padding: UiRect::left(px(pad)), flex_direction: FlexDirection::Column, justify_content: JustifyContent::Center, ..default() },
        UiTransform { scale: Vec2::splat(s), translation: Val2::px(-w / 2.0 * (1.0 - s), 0.0), ..default() },
    ))
    .with_children(content);
}

/// A menu group's own spacer (the main menu's is 42.07 tall, the settings and pause menus' 50).
fn spacer(p: &mut Kids, scene_name: &str, parent: &str) {
    let name = scene::children(scene_name, parent).iter().find(|c| c.eq_ignore_ascii_case("spacer")).unwrap_or_else(|| panic!("no spacer in {parent}"));
    p.spawn(Node { height: px(scene::rect(scene_name, &format!("{parent}/{name}")).size_delta[1]), flex_shrink: 0.0, ..default() });
}

fn back(p: &mut Kids) {
    text_button(p, &scene::label("Menu", "Canvas/Information/Menu/Back"), 28.0, Some(Act::Back), Sel::None, "", menu_node());
}

// ---------------------------------------------------------------- menu pages

fn page(p: &mut Kids, ui: &Ui, settings: &Settings, controls: &Controls, art: &UiArt) {
    match *ui.pages.last().unwrap_or(&Page::Main) {
        Page::Main => {
            side(p, MAIN_MENU_PATH, |s| entries(s, ui));
            // The pause screen shows no logo (Main.unity's Logo(Text) is inactive).
            if ui.mode != Mode::Pause {
                logo(p);
            }
        }
        Page::Maps => maps(p, ui, art),
        Page::Settings => settings_page(p, ui, settings),
        Page::Controls => controls_page(p, ui, controls, art),
        Page::Info => {
            side(p, "Canvas/Information/Menu", back);
            // Info: a vertical layout (middle centre, spacing 16.13) of right aligned texts of fixed heights, then the buttons.
            let pad = scene::padding("Menu", INFO, "VerticalLayoutGroup");
            p.spawn(Node { position_type: PositionType::Absolute, right: px(0), top: px(0), bottom: px(0), width: px(scene::rect("Menu", INFO).size_delta[0]), padding: UiRect { left: px(pad[0]), right: px(pad[1]), top: px(pad[2]), bottom: px(pad[3]) }, justify_content: JustifyContent::Center, align_items: AlignItems::Stretch, ..column(scene::spacing("Menu", INFO, "VerticalLayoutGroup")) }).with_children(|c| {
                let text = |c: &mut Kids, s: &str, size: f32, path: &str| {
                    c.spawn(Node { height: px(scene::rect("Menu", path).size_delta[1]), flex_shrink: 0.0, justify_content: JustifyContent::Center, ..column(0.0) }).with_children(|t| rich(t, s, size, Justify::Right));
                };
                text(c, &scene::text("Menu", INFO_TEXT), scene::font_size("Menu", INFO_TEXT), INFO_TEXT);
                // VersionDisplay.Start
                let bits = if cfg!(target_pointer_width = "64") { "64" } else { "32" };
                let v = super::debug_info::VERSION;
                text(c, &format!("version <b>{}.{}.{} ({bits} bit for Windows)</b>\nusing Unity <b>{UNITY_VERSION}</b>", v._Major, v._Minor, v._Build), 28.0, INFO_VERSION);
                text(c, &scene::text("Menu", INFO_LINKS), scene::font_size("Menu", INFO_LINKS), INFO_LINKS);
                for (text, act) in [("Export game debug info", Act::ExportDebugInfo), ("Open source libraries", Act::Nav(Page::Licenses))] {
                    info_button(c, text, act);
                }
            });
        }
        // Open-source licenses: Back, and a vertical scroll view over the right 1273.69 holding the justified 18 text (padding 15 left, 39 right).
        Page::Licenses => {
            side(p, "Canvas/Open-source licenses/Menu", back);
            p.spawn(Node { position_type: PositionType::Absolute, right: px(0), top: px(0), bottom: px(0), width: px(CANVAS_WIDTH - scene::placed("Menu", "Canvas/Open-source licenses/info container", [0.0, 0.0])[0]), flex_direction: FlexDirection::Column, ..default() }).with_children(|c| {
                scroll_area(c, ui, 4, Node { padding: UiRect { left: px(15), right: px(39), ..default() }, ..scroll_node() }, |s| rich(s, &scene::text("Menu", LICENSES_PATH).replace("\r\n", "\n"), 18.0, Justify::Justified));
            });
        }
        Page::Mods => {
            side(p, "Canvas/Mods/Menu", back);
            mods_page(p, ui, art);
        }
        // Rewrite feature: local achievements (no Steam). Read from the user's Steam schema cache; unlocks are local.
        Page::Achievements => {
            side(p, "Canvas/Information/Menu", back);
            let st = crate::achievements::store();
            let Some(schema) = &st.schema else {
                p.spawn(fill_centered()).with_children(|c| label(c, "No achievement list found: it comes from your Steam's cache of People Playground (appcache/stats).", 28.0, DIM));
                return;
            };
            let got = schema.achievements.iter().filter(|a| st.unlocked(&a.name).is_some()).count();
            let rows: Vec<String> = schema.achievements.iter().map(|a| {
                let when = st.unlocked(&a.name);
                let (title, desc) = if a.hidden && when.is_none() { ("Hidden achievement", "Details are revealed once unlocked") } else { (a.title.as_str(), a.desc.as_str()) };
                let state = match (when, &a.progress) {
                    (Some(_), _) => "<color=#77ff77>Unlocked</color>".to_string(),
                    (None, Some((stat, max))) if !a.hidden => format!("{} / {max}", st.get(stat, false).or(st.get(stat, true)).unwrap_or(0.0).min(*max)),
                    _ => "<color=#888888>Locked</color>".to_string(),
                };
                format!("<b>{title}</b>   {state}
<size=75%>{desc}</size>")
            }).collect();
            let text = format!("<b>Achievements</b>   {got} / {}

{}", schema.achievements.len(), rows.join("

"));
            drop(st);
            p.spawn(Node { position_type: PositionType::Absolute, right: px(0), top: px(0), bottom: px(0), width: px(CANVAS_WIDTH - scene::placed("Menu", "Canvas/Open-source licenses/info container", [0.0, 0.0])[0]), flex_direction: FlexDirection::Column, ..default() }).with_children(|c| {
                scroll_area(c, ui, 9, Node { padding: UiRect { left: px(15), right: px(39), ..default() }, ..scroll_node() }, |s| rich(s, &text, 24.0, Justify::Left));
            });
        }
        // Stats: body (left 250, right and top and bottom 45) holds a vertical Scroll View whose Content (black 0.329, padding
        // and spacing 15) stacks the 64 high UIStatViewBehaviour tiles (black 0.329): the 64 square Icon (preserve aspect),
        // then StatName (white) and the value (green), both 36 and middle left, side by side.
        Page::Stats => {
            side(p, "Canvas/Stats/Menu", back);
            let scene = if ui.mode == Mode::Pause { "Main" } else { "Menu" };
            let tint = |path: &str| scene::image_color(scene, path);
            let content = super::state::stat_tile_path(scene, "");
            let content = content.trim_end_matches('/');
            p.spawn(Node { position_type: PositionType::Absolute, left: px(250), right: px(45), top: px(45), bottom: px(45), flex_direction: FlexDirection::Column, ..default() }).with_children(|c| {
                scroll_area(c, ui, 10, Node { padding: UiRect::all(px(15)), row_gap: px(15), ..scroll_node() }, |s| {
                    s.spawn((Node { position_type: PositionType::Absolute, left: px(0), right: px(0), top: px(0), bottom: px(0), ..default() }, BackgroundColor(tint(content))));
                    for (k, tile) in STAT_TILES.iter().enumerate() {
                        let path = super::state::stat_tile_path(scene, tile);
                        let name = match super::state::stat_view_field(scene, tile, "StatName") {
                            crate::assets::mono::Value::Str(n) => n.clone(),
                            _ => panic!("UIStatViewBehaviour.StatName of {tile}"),
                        };
                        let value = ui.stat_values.get(k).cloned().unwrap_or_default();
                        s.spawn((Node { height: px(64), flex_shrink: 0.0, align_items: AlignItems::Center, ..default() }, BackgroundColor(tint(&path)))).with_children(|t| {
                            t.spawn(Node { width: px(64), height: px(64), flex_shrink: 0.0, ..default() }).with_children(|i| {
                                if let Some(a) = art.named.get(&format!("stat:{scene}:{tile}")) {
                                    picture(i, a, 64.0, 64.0, WHITE);
                                }
                            });
                            label(t, name, 36.0, WHITE);
                            // With Steam the game shows Steam's stats (the rewrite has no Steam).
                            t.spawn((Node::default(), NotCompared("Steam"))).with_children(|v| label(v, value, 36.0, Color::srgb(0.0, 1.0, 0.0)));
                        });
                    }
                });
            });
        }
    }
}

pub const LOGO_FLASH: &str = "Canvas/logo flash";

/// The Menu scene's "logo flash" (the Canvas's last child, so over every page): a 256 square Image (preserve aspect, no
/// raycast) at the canvas centre whose colour LogoFlashBehaviour sets.
#[derive(Component)]
pub struct LogoFlash;

pub fn logo_flash(commands: &mut Commands, art: &UiArt, color: Color) -> Option<Entity> {
    let a = art.named.get("LogoFlash")?;
    let [w, h] = scene::rect("Menu", LOGO_FLASH).size_delta;
    let mut e = commands.spawn((Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, Pickable::IGNORE, GlobalZIndex(11)));
    e.with_children(|p| {
        p.spawn((Node { width: px(w), height: px(h), ..default() }, Pickable::IGNORE)).with_children(|i| picture_with(i, a, w, h, color, LogoFlash));
    });
    Some(e.id())
}

/// Logo(Text): white, a black outline.
fn logo(p: &mut Kids) {
    const TEXT: &str = "people playground";
    const SIZE: f32 = 89.8;
    let at = |x: f32, y: f32| Node { position_type: PositionType::Absolute, right: px(17.9 + x), top: percent(50), margin: UiRect::top(px(y - 55.0)), ..default() };
    for (x, y) in [(-4.0, 0.0), (4.0, 0.0), (0.0, -4.0), (0.0, 4.0), (-3.0, -3.0), (3.0, -3.0), (-3.0, 3.0), (3.0, 3.0)] {
        p.spawn((at(x, y), Pickable::IGNORE, NotCompared("the outline"))).with_children(|c| label(c, TEXT, SIZE, BLACK));
    }
    p.spawn((at(0.0, 0.0), Pickable::IGNORE)).with_children(|c| label(c, TEXT, SIZE, WHITE));
}

/// A MenuController list: the buttons the scene shows, in order, the last one set apart.
fn entries(p: &mut Kids, ui: &Ui) {
    let (table, scene_name, parent) = if ui.mode == Mode::Pause { (PAUSE_MENU, "Main", PAUSE_MENU_PATH) } else { (MAIN_MENU, "Menu", MAIN_MENU_PATH) };
    let shown: Vec<_> = table.iter().filter(|e| e.1).collect();
    for (n, &&(object, _, page, kind)) in shown.iter().enumerate() {
        let path = format!("{parent}/{object}");
        let (name, tip) = (scene::label(scene_name, &path), scene::tooltip(scene_name, &path));
        let name = name.as_str();
        if n + 1 == shown.len() && n > 0 {
            spacer(p, scene_name, parent);
        }
        let in_game = ui.mode == Mode::Pause;
        let act = match (object, page, kind) {
            ("Resume", ..) => Some(Act::Resume),
            ("Restart", ..) => ui.map.as_ref().map(|_| Act::Restart),
            (_, "Map Selection", _) => Some(Act::Nav(Page::Maps)),
            (_, "Settings", _) => Some(Act::Nav(Page::Settings)),
            (_, "Mods", _) => Some(Act::Nav(Page::Mods)),
            (_, "Information", _) => Some(Act::Nav(Page::Info)),
            (_, "Stats", _) => Some(Act::Nav(Page::Stats)),
            (_, _, "quit") => Some(Act::Quit),
            ("Quit", ..) if in_game => Some(Act::ToMenu),
            _ => None,
        };
        // Rewrite feature: local achievements (no Steam), a menu of their own out of the list's layout: to the right of Stats
        // in the pause menu, and of Information in the main menu (whose "Player stats" the scene hides).
        if if in_game { page == "Stats" } else { page == "Information" } {
            p.spawn(Node { flex_shrink: 0.0, ..default() }).with_children(|r| {
                text_button(r, name, 28.0, act, Sel::None, &tip, menu_node());
                let node = Node { position_type: PositionType::Absolute, left: percent(100), top: px(0), ..menu_node() };
                text_button_with(r, "achievements", 28.0, Some(Act::Nav(Page::Achievements)), Sel::None, "Achievements unlocked in this rewrite", node, NotCompared("rewrite feature"));
            });
            continue;
        }
        text_button(p, name, 28.0, act, Sel::None, &tip, menu_node());
    }
}

// PlayButtonMapNameBehaviour.Awake: text and button are the Play button's label and Press below
// PlayButtonMapNameBehaviour.Update: the Play button below ("Enter <map>" in white with the Button interactable, else "Select a chamber" at half white, not interactable)
// PlayMapButtonBehaviour.Awake: text and button are the Play button's label and Press below
// PlayMapButtonBehaviour.Update: the same Play button (its Update is PlayButtonMapNameBehaviour's)
// MapSelectionMenuBehaviour.OnPageSelected: RegenerateView, run each time the page is built
/// MapSelectionMenuBehaviour.RegenerateView: one Map View per MapRegistry map (Maps is empty, AutoAdjustContainerSize off) in the
/// container's GridLayoutGroup (320 × 320 cells, 15 spacing, padding 15 left, top and bottom); UISoundBehaviour.Refresh is the UI sounds'.
// MapViewBehaviour.Awake: presentation (text is the Title label, image the cell's own Image)
// MapViewBehaviour.Start: presentation (text = Map.name on the bar (black at 0.77, the bottom 72.63 of the cell, centred, 28) and image.sprite = Map.Thumbnail)
/// The Mods page (Canvas/Mods). ModListUiBehaviour.UpdateUi makes one ModEntryUiBehaviour per ModLoaderRegistry.Mods entry
/// and shows NoModsFoundText when there are none: the LuaModLoader of 1.28 has none, so the list is the "No mods found" text
/// behind the content. ExistIfSteam (SteamIsNotInitialised) keeps "Steam is not initialised" (SteamworksInitialiser has no
/// Steam). The legacy "Mod entry" template and "Force reload" are inactive.
fn mods_page(p: &mut Kids, ui: &Ui, art: &UiArt) {
    const PANEL: &str = "Canvas/Mods/Panel";
    const LIST: &str = "Canvas/Mods/Panel/mod list";
    const CONTENT: &str = "Canvas/Mods/Panel/mod list/viewport/content";
    const TOOLS: &str = "Canvas/Mods/Panel/tools";
    const LAYOUT: &str = "Canvas/Mods/Panel/tools/layout";
    const DESC: &str = "Canvas/Mods/Panel/tools/mod description container";
    let s = |path: &str| scene::rect("Menu", path).size_delta;
    let color = |path: &str| scene::color_field("Menu", path, "TextMeshProUGUI", "m_fontColor");
    let pad = scene::padding("Menu", PANEL, "HorizontalLayoutGroup");
    let panel = scene::rect("Menu", PANEL);
    let centred = |n: Node| Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..n };
    // A button: its Image tinted by the Button, its centred Title.
    let button = |c: &mut Kids, path: &str, node: Node, act: Act| {
        let title = format!("{path}/Title");
        c.spawn((centred(node), Tint::of("Menu", path).bundle(), Hovered::default(), Press(act))).with_children(|t| label(t, scene::text("Menu", &title), scene::font_size("Menu", &title), color(&title)));
    };
    // ModListUiBehaviour.Awake: UpdateUi listens to ModLoaderRegistry.OnModLoaded (the LuaModLoader loads none)
    // ModListUiBehaviour.OnDestroy: the listener removed
    // ModListUiBehaviour.Update: UpdateUi once (needsUpdate starts true); the page is rebuilt from the registry each time
    // ExistIfSteam..ctor: Behaviour SteamIsInitialised, the scene's is SteamIsNotInitialised
    // ExistIfSteam.Awake: the object stays active by the Behaviour and SteamworksInitialiser.IsInitialised (below)
    let mods = crate::game::mods::registry_mods();
    // ModEntryUiBehaviour.Start
    // ModEntryUiBehaviour.UpdateUi
    // ModEntryUiBehaviour.AddButton
    if !mods.is_empty() {
        unimplemented!("mod-only: ModEntryUiBehaviour (Mod entry.prefab): 1.28's LuaModLoader has no mods, so no entry is made");
    }
    // Panel: stretched right of the Menu column (offset min x 250), a horizontal layout with spacing 15 and padding 15 of
    // the mod list and the tools (flexible width 1.5), laid out at the reference canvas as these widths.
    let row_node = Node { position_type: PositionType::Absolute, left: px(-panel.size_delta[0]), right: px(0), top: px(0), bottom: px(0), padding: UiRect { left: px(pad[0]), right: px(pad[1]), top: px(pad[2]), bottom: px(pad[3]) }, ..row(scene::spacing("Menu", PANEL, "HorizontalLayoutGroup")) };
    p.spawn(Node { align_items: AlignItems::Stretch, ..row_node }).with_children(|r| {
        // mod list: NoModsFoundText on the ScrollRect object, drawn behind the viewport.
        r.spawn(Node { flex_grow: s(LIST)[0], flex_basis: px(0), height: percent(100), flex_direction: FlexDirection::Column, ..default() }).with_children(|l| {
            l.spawn(Node { position_type: PositionType::Absolute, left: px(0), right: px(0), top: px(0), bottom: px(0), ..centred(default()) }).with_children(|t| label(t, scene::text("Menu", LIST), scene::font_size("Menu", LIST), color(LIST)));
            let cpad = scene::padding("Menu", CONTENT, "VerticalLayoutGroup");
            scroll_area(l, ui, 5, Node { padding: UiRect { left: px(cpad[0]), right: px(cpad[1]), top: px(cpad[2]), bottom: px(cpad[3]) }, row_gap: px(scene::spacing("Menu", CONTENT, "VerticalLayoutGroup")), ..scroll_node() }, |c| {
                // The content's vertical layout places its children in sibling order.
                for child in scene::children("Menu", CONTENT) {
                    match child.as_str() {
                        "Filter" => {
                        // Filter: the input field (offset max x -55.9) and the reset button at its right.
                        const FILTER: &str = "Canvas/Mods/Panel/mod list/viewport/content/Filter";
                        let field = format!("{FILTER}/Input field");
                        let reset = s(&format!("{FILTER}/reset"));
                        c.spawn(Node { height: px(s(FILTER)[1]), flex_shrink: 0.0, ..row(0.0) }).with_children(|f| {
                            let w = s(&field)[0];
                            f.spawn((Node { height: percent(100), flex_grow: 1.0, align_items: AlignItems::Center, ..default() }, Tint::of_graphic("Menu", &field, "TMP_InputField", &format!("{field}/Text Area")).bundle(), Hovered::default(), Press(Act::ModsFilter))).with_children(|t| {
                                let holder = format!("{field}/Text Area/Placeholder");
                                if ui.mods_filter.is_empty() && !ui.mods_typing {
                                    label(t, scene::text("Menu", &holder), scene::font_size("Menu", &holder), color(&holder));
                                } else {
                                    label(t, format!("{}{}", ui.mods_filter, if ui.mods_typing { "_" } else { "" }), scene::font_size("Menu", &holder), WHITE);
                                }
                            });
                            let at = scene::rect("Menu", &format!("{FILTER}/reset")).anchored_position[0];
                            f.spawn(Node { width: px(-w), flex_shrink: 0.0, justify_content: JustifyContent::FlexEnd, padding: UiRect::right(px(-at)), ..row(0.0) }).with_children(|z| {
                                z.spawn((Node { width: px(reset[0]), height: px(reset[1]), flex_shrink: 0.0, ..default() }, Hovered::default(), Press(Act::ModsFilterReset))).with_children(|b| {
                                    if let Some(a) = art.named.get("ModsFilterReset") {
                                        picture(b, a, reset[0], reset[1], WHITE);
                                    }
                                });
                            });
                        });
                        }
                        "Text (TMP)" => {
                        let steam = format!("{CONTENT}/Text (TMP)");
                        if !crate::game::mods::STEAM_UGC {
                            // The game run with Steam has it inactive.
                            c.spawn((Node { height: px(s(&steam)[1]), flex_shrink: 0.0, ..centred(default()) }, NotCompared("Steam"))).with_children(|t| label(t, scene::text("Menu", &steam), scene::font_size("Menu", &steam), color(&steam)));
                        }
                        }
                        _ if child.starts_with("quality of life") => {
                        const QOL: &str = "Canvas/Mods/Panel/mod list/viewport/content/quality of life! I hate this game.";
                        c.spawn(Node { height: px(s(QOL)[1]), flex_shrink: 0.0, align_items: AlignItems::Stretch, ..row(scene::spacing("Menu", QOL, "HorizontalLayoutGroup")) }).with_children(|q| {
                            for (name, on) in [("Enable all ", true), ("Disable all", false)] {
                                button(q, &format!("{QOL}/{name}"), Node { flex_grow: 1.0, flex_basis: px(0), ..default() }, Act::ModsSetAll(on));
                            }
                        });
                        }
                        _ => {}
                    }
                }
            });
        });
        // tools: the mod description scroll view over the layout (height 219.4) at the bottom.
        r.spawn(Node { flex_grow: s(TOOLS)[0], flex_basis: px(0), height: percent(100), flex_direction: FlexDirection::Column, ..default() }).with_children(|t| {
            t.spawn(Node { flex_grow: 1.0, min_height: px(0), flex_direction: FlexDirection::Column, ..default() }).with_children(|d| {
                scroll_area(d, ui, 6, scroll_node(), |c| {
                    // content (black 0.18) fitted to "mod desc": <align=center><alpha=#50>No mod selected.
                    let desc = format!("{DESC}/viewport/content/mod desc");
                    let text = scene::text("Menu", &desc).replace("<align=center>", "").replace("<alpha=#50>", "");
                    c.spawn((Node { flex_shrink: 0.0, justify_content: JustifyContent::Center, ..default() }, BackgroundColor(scene::image_color("Menu", &format!("{DESC}/viewport/content"))))).with_children(|b| label(b, text, scene::font_size("Menu", &desc), Color::srgba(1.0, 1.0, 1.0, 80.0 / 255.0)));
                });
            });
            t.spawn((Node { height: px(s(LAYOUT)[1]), flex_shrink: 0.0, flex_direction: FlexDirection::Column, justify_content: JustifyContent::Center, ..default() }, BackgroundColor(scene::image_color("Menu", LAYOUT)))).with_children(|l| {
                const BUTTONS: &str = "Canvas/Mods/Panel/tools/layout/buttons";
                let cell = scene::vec2("Menu", BUTTONS, "GridLayoutGroup", "m_CellSize");
                let gap = scene::vec2("Menu", BUTTONS, "GridLayoutGroup", "m_Spacing");
                let gpad = scene::padding("Menu", BUTTONS, "GridLayoutGroup");
                // The layout's children (and the grid's buttons) in sibling order.
                for child in scene::children("Menu", LAYOUT) {
                    if child == "buttons" {
                        l.spawn(Node { height: px(s(BUTTONS)[1]), flex_shrink: 0.0, padding: UiRect::left(px(gpad[0])), justify_content: JustifyContent::Center, ..row(gap[0]) }).with_children(|g| {
                            for name in scene::children("Menu", BUTTONS).iter().filter(|n| scene::active("Menu", &format!("{BUTTONS}/{n}"))) {
                                let act = if name.starts_with("Open Mods folder") { Act::ModsOpenFolder } else { Act::ModsOpenDocs };
                                button(g, &format!("{BUTTONS}/{name}"), Node { width: px(cell[0]), height: px(cell[1]), flex_shrink: 0.0, ..default() }, act);
                            }
                        });
                    } else if child == "title (2)" {
                        // title (2): margins 15, top left.
                        let warn = format!("{LAYOUT}/title (2)");
                        l.spawn(Node { height: px(s(&warn)[1]), flex_shrink: 0.0, padding: UiRect::all(px(15)), ..default() }).with_children(|w| label(w, scene::text("Menu", &warn), scene::font_size("Menu", &warn), color(&warn)));
                    }
                }
            });
        });
    });
}

fn maps(p: &mut Kids, ui: &Ui, art: &UiArt) {
    const SELECT: &str = "Canvas/Map Selection";
    let menu = scene::placed("Menu", &format!("{SELECT}/Map menu"), [0.0, 0.0]);
    let inset = scene::placed("Menu", &format!("{SELECT}/Map menu/Scroll View"), [0.0, 0.0]);
    let play = scene::rect("Menu", &format!("{SELECT}/Play button"));
    side(p, &format!("{SELECT}/Menu"), back);
    // Map menu's rect and its Scroll View's inset.
    p.spawn((Node { position_type: PositionType::Absolute, left: px(menu[0]), right: px(-(menu[0] + menu[2])), top: px(menu[1]), bottom: px(-(menu[1] + menu[3])), padding: UiRect { left: px(inset[0]), right: px(-(inset[0] + inset[2])), top: px(inset[1]), bottom: px(-(inset[1] + inset[3])) }, ..column(0.0) }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.33)))).with_children(|m| {
        if let Some(e) = &ui.error {
            label(m, format!("Could not load: {e}"), 28.0, WARN);
        }
        scroll_area(m, ui, 0, scroll_node(), |s| {
            s.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(15), row_gap: px(15), padding: UiRect { left: px(15), right: px(0), top: px(15), bottom: px(15) }, flex_shrink: 0.0, ..default() }).with_children(|g| {
                for (i, map) in ui.maps.iter().enumerate() {
                    let tile = Node { width: px(320), height: px(320), flex_shrink: 0.0, align_items: AlignItems::FlexEnd, ..default() };
                    g.spawn((tile, BackgroundColor(CLEAR), Hovered::default(), Look { sel: Sel::Map(i), on: true }, Press(Act::SelectMap(i)))).with_children(|t| {
                        if let Some(a) = art.maps.get(&map.name) {
                            picture(t, a, 320.0, 320.0, WHITE);
                        }
                        // The bar: anchors stretched, sizeDelta.y -247.3658 and anchoredPosition.y -123.68286 in the cell.
                        let bar = Node { position_type: PositionType::Absolute, left: px(0), right: px(0), bottom: px(0), height: px(320.0 - 247.3658), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() };
                        t.spawn((bar, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.77254903)), Pickable::IGNORE)).with_children(|n| label(n, map.name.clone(), 28.0, WHITE));
                    });
                }
            });
        });
    });
    // Play button.
    let name = ui.selected.and_then(|i| ui.maps.get(i)).map(|m| m.name.as_str());
    let act = name.map(|_| Act::Enter);
    let on = act.is_some();
    let k = play.scale[0];
    let mut e = p.spawn((
        // Its local scale grows it from the bottom right pivot, its title with it.
        Node { position_type: PositionType::Absolute, right: px(-play.anchored_position[0]), bottom: px(play.anchored_position[1]), width: px(play.size_delta[0] * k), height: px(play.size_delta[1] * k), padding: UiRect::left(px(20.0 * k)), align_items: AlignItems::Center, ..default() },
        BackgroundColor(BLACK),
        Hovered::default(),
    ));
    if let Some(a) = act {
        e.insert(Press(a));
    }
    e.with_children(|c| label(c, name.map_or(crate::il!(str "PlayButtonMapNameBehaviour::Update" 1).to_string(), |n| format!("Enter {n}")), scene::font_size("Menu", &format!("{SELECT}/Play button/Title")) * k, if on { WHITE } else { DIM }));
}

// SettingsGeneratorBehaviour.Start: Generate when the page is built
/// SettingsGeneratorBehaviour.Generate: every Preferences field with a Setting attribute and no HideInSettingsMenu, in the Sort order
/// (`tab_rows`), a template each in its category's tab; enum, bool, option and number fields get the widgets below.
fn settings_page(p: &mut Kids, ui: &Ui, settings: &Settings) {
    side(p, "Canvas/Settings/Menu", |s| {
        for &(object, key) in SETTINGS_MENU {
            let name = &if key == "Mods" { "Mods".to_string() } else { scene::label("Menu", &format!("Canvas/Settings/Menu/{object}")) };
            match key {
                "Controls" => text_button(s, name, 28.0, Some(Act::Nav(Page::Controls)), Sel::None, "", menu_node()),
                "Back" => {
                    spacer(s, "Menu", "Canvas/Settings/Menu");
                    back(s);
                }
                k => {
                    let c = category_index(k);
                    text_button(s, name, 28.0, Some(Act::Tab(c)), Sel::Tab(c), "", menu_node());
                }
            }
        }
    });
    p.spawn(Node { position_type: PositionType::Absolute, left: px(277.8), right: px(30), top: px(25), bottom: px(25), ..column(0.0) }).with_children(|c| {
        // Content: a vertical layout, spacing 20, padding right 7.
        scroll_area(c, ui, 1, Node { row_gap: px(20), padding: UiRect::right(px(7)), ..scroll_node() }, |s| {
            for i in tab_rows(ui.tab) {
                setting_row(s, i, settings);
            }
        });
    });
}

// SettingsGeneratorBehaviour.GenerateBoolSetting: Enabled and Disabled radio buttons
// SettingsGeneratorBehaviour.GenerateEnumSetting: one radio button per name
// SettingsGeneratorBehaviour.CreateRadioButton: a button that sets its value and checks itself (Act::Set, `Checked`)
// SettingsGeneratorBehaviour.GenerateNumberSetting: the slider (SliderOf; Settings::set_fraction snaps and rounds) and its text
// SettingTemplateBehaviour.SetTitle: Label.text = the title (`bold(m, def.title, ..)` in setting_row).
// SettingTemplateBehaviour.SetDescription: Description shown with the text, or its GameObject inactive when IsNullOrWhiteSpace (setting_row).
/// SettingsGeneratorBehaviour.CreateNewSetting: the setting template, the name over its control on the left, the description on the right.
fn setting_row(s: &mut Kids, i: usize, settings: &Settings) {
    let def = &SETTINGS[i];
    let translucent = Color::srgba(0.0, 0.0, 0.0, 0.15);
    // The template: a horizontal layout (padding 5, spacing 5) of Main (flexible 569.8, a vertical layout of padding 5 and
    // spacing 18.5) and the Description (preferred 1300, flexible 1 by the force expand): the Description keeps its 1300
    // and the width over it goes 569.8 : 1 to them.
    s.spawn((Node { width: percent(100), min_height: px(119.9), flex_shrink: 0.0, padding: UiRect::all(px(5)), column_gap: px(5), ..default() }, BackgroundColor(translucent))).with_children(|r| {
        r.spawn(Node { flex_basis: px(0), flex_grow: 569.8, min_width: px(0), padding: UiRect::all(px(5)), ..column(18.5) }).with_children(|m| {
            // Setting name keeps its rect's height (the layout does not control heights).
            m.spawn(Node { height: px(scene::rect("SettingTemplate", "Main/Setting name").size_delta[1]), flex_shrink: 0.0, ..default() }).with_children(|n| bold(n, def.title, 28.0, WHITE));
            m.spawn((Node { height: px(50), width: percent(100), flex_shrink: 0.0, ..row(0.0) }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.33)))).with_children(|w| {
                let choice = |w: &mut Kids, text: String, v: f32| {
                    let node = Node { flex_grow: 1.0, flex_basis: px(0), height: percent(100), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() };
                    let sel = Sel::Setting(i, v);
                    w.spawn((node, BackgroundColor(CLEAR), Hovered::default(), Look { sel: Sel::None, on: true }, Press(Act::Set(i, v)))).with_children(|c| {
                        c.spawn((Text::new(text), font(28.0), TextColor(WHITE), Pickable::IGNORE, Checked(sel)));
                    });
                };
                match def.kind {
                    Kind::Bool => {
                        choice(w, "Enabled".into(), 1.0);
                        choice(w, "Disabled".into(), 0.0);
                    }
                    Kind::Enum(names) => {
                        for (n, name) in names.iter().enumerate() {
                            choice(w, name.to_string(), n as f32);
                        }
                    }
                    Kind::Options(options) => {
                        for &o in options {
                            let mut t = settings.clone();
                            t.values[i] = o;
                            choice(w, t.text(i), o);
                        }
                    }
                    Kind::Int { .. } | Kind::Float { .. } => {
                        w.spawn((Node { width: percent(100), height: percent(100), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.06)), SliderOf(i), Hovered::default(), RelativeCursorPosition::default())).with_children(|track| {
                            track
                                .spawn((Node { width: percent(settings.fraction(i) * 100.0), height: percent(100), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.25)), FillOf(i), Pickable::IGNORE))
                                .with_children(|f| {
                                    f.spawn((Node { position_type: PositionType::Absolute, right: px(0), top: px(0), bottom: px(0), width: px(20), ..default() }, BackgroundColor(WHITE), Pickable::IGNORE));
                                });
                            track.spawn((Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..absolute() }, Pickable::IGNORE)).with_children(|t| bound(t, Bound::Setting(i), 28.0, WHITE));
                        });
                    }
                }
            });
        });
        r.spawn(Node { flex_grow: 1.0, flex_basis: px(1300), min_width: px(0), ..column(4.0) }).with_children(|d| {
            if !def.description.trim().is_empty() {
                label(d, def.description.replace("\\n", "\n"), 28.0, WHITE);
            }
        });
    });
}

/// ControlSchemeEditorBehaviour.InitialiseControlScheme, ControlSchemeEditorBehaviour.CreateControlObject and ControlSchemeEditorBehaviour.Awake: a header per ActionCategory
/// (MapEditor skipped; Awake builds it after LoadInputSystem), then a row per visible action in a category with its name and trigger editor.
fn controls_page(p: &mut Kids, ui: &Ui, controls: &Controls, art: &UiArt) {
    side(p, "Canvas/Controls/Menu", back);
    p.spawn(Node { position_type: PositionType::Absolute, left: px(277.8), right: px(30), top: px(25), bottom: px(25), ..column(0.0) }).with_children(|c| {
        // Content: a VerticalLayoutGroup (padding and spacing 15) of the 66.9 high Header and Control prefabs at full width.
        scroll_area(c, ui, 2, Node { padding: UiRect::all(px(15)), row_gap: px(15), ..scroll_node() }, |s| {
            for &cat in ACTION_CATEGORIES.iter() {
                let rows: Vec<usize> = ACTIONS.iter().enumerate().filter(|(_, a)| a.category == cat && !a.invisible).map(|(i, _)| i).collect();
                if rows.is_empty() {
                    continue;
                }
                // Header: white at 0.043, padding 15 left and right, the Title (bold 40) middle left.
                s.spawn((Node { flex_shrink: 0.0, height: px(66.9), padding: UiRect::horizontal(px(15)), align_items: AlignItems::Center, ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.043137256)))).with_children(|t| bold(t, spaced(cat), 40.0, WHITE));
                for i in rows {
                    let a = ACTIONS[i];
                    // Control: black at 0.239, a HorizontalLayoutGroup (padding 10 left and right, spacing 10, middle left) of
                    // the Action text (636.04 wide), the Trigger (flexible) and the 50 wide Unbind button.
                    s.spawn((Node { flex_shrink: 0.0, height: px(66.9), padding: UiRect::horizontal(px(10)), ..row(10.0) }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.23921569)))).with_children(|r| {
                        r.spawn(Node { width: px(636.04), flex_shrink: 0.0, ..default() }).with_children(|c| label(c, a.name, 28.0, WHITE));
                        r.spawn((
                            Node { flex_grow: 1.0, flex_basis: px(0), height: px(50), justify_content: JustifyContent::FlexStart, align_items: AlignItems::Center, ..default() },
                            BackgroundColor(CLEAR),
                            Hovered::default(),
                            Look { sel: Sel::Bind(i), on: true },
                            Press(Act::Rebind(i)),
                            Tip("Click, then press the key (hold another key first for a modifier). Moving the mouse far cancels.".into(), false),
                        ))
                        .with_children(|b| bound(b, Bound::Bind(i), 28.0, WHITE));
                        // TriggerEditorBehaviour.SetText: UnbindButton (its img the Delete sprite) active unless Key is None.
                        if controls.binds[i].0 != "None" {
                            // Unbind: black at 0.533, its img the Delete sprite at 0.6 scale.
                            r.spawn((Node { width: px(50), height: px(50), flex_shrink: 0.0, justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.53333336)), Hovered::default(), Look { sel: Sel::None, on: true }, Tip("Unbind".into(), false), Press(Act::Unbind(i)))).with_children(|x| {
                                if let Some(a) = art.named.get("Delete") {
                                    picture(x, a, 30.0, 30.0, WHITE);
                                }
                            });
                        }
                    });
                }
            }
        });
    });
}

// ---------------------------------------------------------------- the game's HUD

fn hud(root: &mut Kids, ui: &Ui, settings: &Settings, controls: &Controls, art: &UiArt, stopped: Option<&str>) {
    status_bar(root, ui, art);
    if !ui.hide_panels {
        catalog(root, ui, art, catalog_width(ui, settings));
        if let Some(values) = ui.env_values.filter(|_| super::ENV_REGEN.swap(false, std::sync::atomic::Ordering::Relaxed)) {
            environment_regenerated(&values, settings.get("TemperatureUnit") as i32);
        }
        if ui.env_open {
            environment(root, ui, settings);
        }
        toolbar(root, ui, controls, art);
    }
    selection_box(root, ui, art);
    if let Some(([x, y], layer, order)) = &ui.layer {
        layer_editor(root, *x, *y, layer, order);
    }
    // NotificationArea (Main scene): full screen, its Image disabled; a VerticalLayoutGroup, lower right, padding right
    // 15 and bottom 15, spacing 4, children keep their own size. notes_update fills it.
    root.spawn((
        Node { position_type: PositionType::Absolute, left: px(0), right: px(0), top: px(0), bottom: px(0), flex_direction: FlexDirection::Column, justify_content: JustifyContent::FlexEnd, align_items: AlignItems::FlexEnd, row_gap: px(4), padding: UiRect { right: px(15), bottom: px(15), ..default() }, ..default() },
        NoteArea,
        Pickable::IGNORE,
    ));
    if let Some(m) = stopped {
        root.spawn(fill_centered()).insert(Pickable::IGNORE).with_children(|c| {
            c.spawn((Node { padding: UiRect::all(px(30)), align_items: AlignItems::Center, ..column(16.0) }, BackgroundColor(BLACK))).with_children(|b| {
                bold(b, "The game stopped", 28.0, WARN);
                label(b, m, 22.0, WHITE);
                b.spawn(row(40.0)).with_children(|r| {
                    let node = || Node { width: px(160), height: px(50), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() };
                    text_button(r, "Reset", 28.0, ui.map.as_ref().map(|_| Act::Restart), Sel::None, "", node());
                    text_button(r, "Menu", 28.0, Some(Act::ToMenu), Sel::None, "", node());
                });
            });
        });
    }
    // DialogBoxManager.TextEntry: the message, the input field (its placeholder while empty), Apply and Cancel.
    // The input field: 80 high, Image tinted 0.03, 33.1 centred text; the selection is drawn green (as measured in the
    // game: TMP's SelectionColor (0, 1, 0, 0.09) as it blends), the caret 0.196 grey.
    if let Some(e) = &ui.entry {
        dialog_box(root, |b| {
            dialog_title(b, e.title.clone(), 36.0);
            b.spawn((Node { height: px(52), margin: UiRect::axes(px(10), px(14)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..row(0.0) }, BackgroundColor(Color::srgb(0.03, 0.03, 0.03)))).with_children(|f| {
                if e.text.is_empty() {
                    label(f, e.placeholder.clone(), 33.1, Color::srgba(1.0, 1.0, 1.0, 0.043137256));
                } else if ui.entry_all {
                    f.spawn((Text::new(e.text.clone()), font(33.1), TextColor(WHITE), BackgroundColor(Color::srgb_u8(12, 87, 12)), Pickable::IGNORE));
                } else {
                    label(f, e.text.clone(), 33.1, WHITE);
                    // The caret is a Graphic (TMP_SelectionCaret), not text.
                    f.spawn((Node { width: px(2), height: px(36), ..default() }, BackgroundColor(Color::srgb(0.19607843, 0.19607843, 0.19607843)), Pickable::IGNORE));
                }
            });
            // ContextMenuBehaviour.SaveAction's TextEntry: Cancel, then Save.
            if e.targets.iter().any(|t| matches!(t, crate::game::menu::Entry::SaveContraption)) {
                dialog_buttons(b, Node { height: px(80), ..default() }, 30.23, [("Cancel", Act::EntryCancel), ("Save", Act::EntryApply)]);
            } else {
                dialog_buttons(b, Node { height: px(80), ..default() }, 30.23, [(&apply(), Act::EntryApply), (&cancel(), Act::EntryCancel)]);
            }
        });
    }
    // ExportGameDebugInfoBehaviour.Export's DialogBoxManager.Dialog (SetHeight 400) or Notification: the title and the buttons.
    // The boxes under the top one first (each drawn over the one before), then the top.
    for (k, d) in ui.under.iter().enumerate().map(|(k, d)| (Some(k), d)).chain(ui.dialog.iter().map(|d| (None, d))) {
        let press = |i: usize| k.map_or(Act::DialogButton(i), |k| Act::DialogUnder(k, i));
        dialog_box(root, |b| {
            // The text's preferred height (the box's ContentSizeFitter sizes it so); the buttons stay at the bottom.
            b.spawn(Node { height: px(tmp_preferred_height(&d.title, 36.0)), min_height: px(0), flex_grow: 1.0, flex_shrink: 0.0, ..column(0.0) }).with_children(|t| rich(t, &d.title, 36.0, Justify::Center));
            match d.buttons.as_slice() {
                [a] => dialog_buttons(b, Node { height: px(80), ..default() }, 30.23, [(&a.0, press(0))]),
                [a, c, e] => dialog_buttons(b, Node { height: px(80), ..default() }, 30.23, [(&a.0, press(0)), (&c.0, press(1)), (&e.0, press(2))]),
                [a, c] => dialog_buttons(b, Node { height: px(80), ..default() }, 30.23, [(&a.0, press(0)), (&c.0, press(1))]),
                _ => unreachable!("a dialog with one to three buttons"),
            }
        });
    }
    // PianoDialogBox ("Bell dialog box", Main scene): 1874 x 457, the title, the piano of 28 white keys (C 3 to B 6)
    // with the black keys over their top 134 pixels, then Cancel and Apply. A key is green while it is SelectedBellClip.
    if let Some(pi) = &ui.piano {
        use crate::game::menu::{piano_key_label, piano_keys};
        // WhiteKeyContainer: 28 keys with spacing 6; each black key 0.6 of a slot, centred on the gap after its white key.
        let holder = format!("{DIALOG_BOX}/Piano holder");
        let boxr = scene::rect("Main", DIALOG_BOX);
        let piano_at = scene::placed("Main", &format!("{holder}/Piano"), boxr.size_delta);
        let gap = scene::spacing("Main", &format!("{holder}/Piano/WhiteKeyContainer"), "HorizontalLayoutGroup");
        let black_height = piano_at[3] + scene::rect("Main", &format!("{holder}/Piano/BlackKeyContainer")).size_delta[1];
        let black_tint = scene::button_color("Main", &format!("{holder}/BlackKey/button"), "m_NormalColor")[0];
        let w = (piano_at[2] - 27.0 * gap) / 28.0;
        let keys = piano_keys();
        let lit = |k: usize| pi.key_lit(k);
        let abs = |top: f32, h: f32| Node { position_type: PositionType::Absolute, left: px(10), right: px(10), top: px(top), height: px(h), ..default() };
        root.spawn(fill_centered()).with_children(|c| {
            c.spawn((Node { width: px(boxr.size_delta[0]), height: px(boxr.size_delta[1]), top: px(-boxr.anchored_position[1]), ..default() }, BackgroundColor(BLACK), dialog_outline())).with_children(|b| {
                b.spawn((Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..abs(10.0, 100.0) }, Pickable::IGNORE)).with_children(|t| tmp_bold(t, "Choose bell note", 32.0, WHITE));
                b.spawn(Node { left: px(piano_at[0]), width: px(piano_at[2]), ..abs(115.0, piano_at[3]) }).with_children(|piano| {
                    for (k, (_, key)) in keys.iter().enumerate() {
                        let black = PIANO_BLACK[*key];
                        let (left, width, height) = if black { (white_before(&keys, k) as f32 * (w + gap) - gap / 2.0 - w * 0.3, w * 0.6, black_height) } else { (k as f32 * (w + gap), w, piano_at[3]) };
                        // UiPianoKeyBehaviour: the target graphic green while selected, else white; times the button's normal tint.
                        let tint = if black { black_tint } else { 1.0 };
                        let colour = if lit(k) { Color::srgb(0.0, tint, 0.0) } else { Color::srgb(tint, tint, tint) };
                        piano.spawn((Node { position_type: PositionType::Absolute, left: px(left), top: px(0), width: px(width), height: px(height), justify_content: JustifyContent::Center, align_items: AlignItems::FlexEnd, padding: UiRect::bottom(px(1.5)), ..default() }, BackgroundColor(colour), Hovered::default(), Press(Act::PianoKey(k)))).with_children(|t| {
                            piano_label(t, &piano_key_label(*key), if black { 19.0 } else { 22.0 }, if black { WHITE } else { BLACK }, art.bravura.as_ref());
                        });
                    }
                });
                dialog_buttons(b, abs(365.0, 81.0), 32.0, [(&cancel(), Act::PianoCancel), (&apply(), Act::PianoApply)]);
            });
        });
    }
    // LimbStatusViewBehaviour's panel (Main scene): 552 x 275 at the top right, the four bars on the left, the blood
    // information, the damage source stats at the bottom right and the close button.
    if let Some((stats, blood, bars)) = &ui.limb_view {
        let abs = |left: f32, top: f32, w: f32, h: f32| Node { position_type: PositionType::Absolute, left: px(left), top: px(top), width: px(w), height: px(h), ..default() };
        const LV: &str = "Canvas/Limb status view";
        let panel_rect = scene::rect("Main", LV);
        let size = panel_rect.size_delta;
        let at = |name: &str| scene::placed("Main", &format!("{LV}/{name}"), size);
        let abs_at = |name: &str| {
            let r = at(name);
            abs(r[0], r[1], r[2], r[3])
        };
        let bars_pad = scene::padding("Main", &format!("{LV}/bars"), "HorizontalLayoutGroup");
        let lab = scene::rect("Main", &format!("{LV}/bars/shock/label")).size_delta;
        let label_size = scene::font_size("Main", &format!("{LV}/bars/shock/label"));
        let close = scene::rect("Main", &format!("{LV}/close"));
        root.spawn((Node { position_type: PositionType::Absolute, right: px(-panel_rect.anchored_position[0]), top: px(-panel_rect.anchored_position[1] - (1.0 - panel_rect.pivot[1]) * size[1]), width: px(size[0]), height: px(size[1]), ..default() }, BackgroundColor(scene::image_color("Main", LV)))).with_children(|panel| {
            panel.spawn(Node { flex_direction: FlexDirection::Row, padding: UiRect::all(px(bars_pad[0])), column_gap: px(scene::spacing("Main", &format!("{LV}/bars"), "HorizontalLayoutGroup")), ..abs_at("bars") }).with_children(|row| {
                let looks = ["consciousness", "shock", "oxygen", "adrenaline"];
                for (k, bar) in looks.into_iter().enumerate() {
                    let name = scene::text("Main", &format!("{LV}/bars/{bar}/label"));
                    row.spawn((Node { flex_grow: 1.0, flex_basis: px(0), height: percent(100), justify_content: JustifyContent::FlexEnd, flex_direction: FlexDirection::Column, ..default() }, BackgroundColor(scene::image_color("Main", &format!("{LV}/bars/{bar}"))))).with_children(|col| {
                        col.spawn((Node { width: percent(100), height: percent(100.0 * bars[k].clamp(0.0, 1.0)), ..default() }, BackgroundColor(scene::image_color("Main", &format!("{LV}/bars/{bar}/bar")))));
                        // The label (Bold | UpperCase, left aligned) turned 90 degrees about its centre: it reads up from
                        // the bar's bottom.
                        col.spawn((Node { position_type: PositionType::Absolute, left: percent(50), margin: UiRect::left(px(-lab[0] / 2.0)), bottom: px(4.0 + lab[0] / 2.0 - lab[1] / 2.0), width: px(lab[0]), height: px(lab[1]), align_items: AlignItems::Center, ..default() }, UiTransform { rotation: Rot2::degrees(-90.0), ..default() }, Pickable::IGNORE)).with_children(|l| {
                            l.spawn((Text::new(name.trim_end().to_uppercase()), font(label_size), TextColor(WHITE), TextShadow { offset: Vec2::new(0.6, 0.0), color: WHITE }, TextLayout { linebreak: LineBreak::NoWrap, ..default() }, Pickable::IGNORE));
                        });
                    });
                }
            });
            panel.spawn((Node { justify_content: JustifyContent::Center, ..abs_at("blood information") }, Pickable::IGNORE)).with_children(|b| {
                b.spawn((Node { flex_direction: FlexDirection::Column, justify_content: JustifyContent::Center, ..default() },)).with_children(|c| small_caps(c, &blood.replace('\r', ""), scene::font_size("Main", &format!("{LV}/blood information")), WHITE, 22.6));
            });
            panel.spawn((Node { flex_direction: FlexDirection::Column, justify_content: JustifyContent::FlexEnd, align_items: AlignItems::FlexEnd, ..abs_at("limb damage source stats") }, Pickable::IGNORE)).with_children(|c| {
                for (k, (s, color)) in [(&stats[0], WHITE), (&stats[1], Color::srgb_u8(0xFF, 0x55, 0x55)), (&stats[2], WHITE)].into_iter().enumerate() {
                    if !s.is_empty() {
                        let shadow = if k == 0 { 0.6 } else { 0.0 };
                        c.spawn((Text::new(s.replace('\r', "").trim_end_matches('\n').to_lowercase()), font(22.0), TextColor(color), TextShadow { offset: Vec2::new(shadow, 0.0), color: if k == 0 { color } else { CLEAR } }, TextLayout { justify: Justify::Right, ..default() }, Pickable::IGNORE));
                    }
                }
            });
            panel.spawn((Node { position_type: PositionType::Absolute, right: px(-close.anchored_position[0]), top: px(-close.anchored_position[1]), width: px(close.size_delta[0]), height: px(close.size_delta[1]), ..default() }, Hovered::default(), Press(Act::LimbViewClose))).with_children(|x| {
                if let Some(a) = art.named.get("Delete") {
                    picture(x, a, close.size_delta[0], close.size_delta[1], WHITE);
                }
            });
        });
    }
    // DialogBoxManager.KeyEntry, the top one: the title, the key button (KeyTriggerEditor), Apply and Cancel.
    if let Some(k) = ui.key_boxes.last() {
        dialog_box(root, |b| {
            dialog_title(b, crate::il!(str "KeyTriggerBehaviour::<Start>b__12_0" 0), 36.0);
            // Control: 80 high, Image black at 0.239, the Trigger text bold 28; the button is not interactable while it waits.
            let mut e = b.spawn((Node { height: px(80), padding: UiRect::axes(px(15), px(0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.23921569)), Hovered::default()));
            if k.wait.is_none() {
                e.insert(Press(Act::KeyWait));
            }
            // The Trigger text is Bold | UpperCase (fontStyle 17).
            e.with_children(|t| tmp_bold(t, k.text().to_uppercase(), 28.0, WHITE));
            dialog_buttons(b, Node { height: px(80), ..default() }, 30.23, [(&apply(), Act::KeyApply), (&cancel(), Act::KeyCancel)]);
        });
    }
    // ColorpickerDialogBehaviour ("Colorpicker", Main scene): 615.17 x 873.14 black with the dialog Outline; Title (bold
    // 32) and Message (28, top left) bands of 84.65; the picker: the outline square (white 0.141), the saturation/value
    // box, the hue box with its 4 px line, the preview and the hex field (scaled 1.05, 24); the button holder (padding
    // 15, spacing 15) of image 0.2264 at 0.761 tinted (0, 0, 0, 0.5) with bold 32 titles.
    if let Some(cb) = &ui.colour {
        const CP: &str = "Canvas/Dialog container/Colorpicker";
        const PICKER: &str = "Canvas/Dialog container/Colorpicker/Colourpicker";
        const FIELD: &str = "Canvas/Dialog container/Colorpicker/Colourpicker/InputField (TMP)";
        let boxr = scene::rect("Main", CP).size_delta;
        let in_box = |name: &str| scene::placed("Main", &format!("{CP}/{name}"), boxr);
        let picker = in_box("Colourpicker");
        let in_picker = |name: &str| scene::placed("Main", &format!("{PICKER}/{name}"), [picker[2], picker[3]]);
        let sv_box = in_picker("Saturation Value box");
        let hue_box = in_picker("Hue box");
        let side = sv_box[2];
        let [h, s, v] = cb.hsv;
        let rgb = cb.rgb();
        let srgb = |c: [f32; 4]| Color::srgb(c[0], c[1], c[2]);
        let linear = |angle: f32, stops: Vec<ColorStop>| BackgroundGradient::from(LinearGradient { color_space: InterpolationColorSpace::Srgba, angle, stops });
        let at = |left: f32, top: f32, w: f32, hgt: f32| Node { position_type: PositionType::Absolute, left: px(left), top: px(top), width: px(w), height: px(hgt), ..default() };
        let at_r = |r: [f32; 4]| at(r[0], r[1], r[2], r[3]);
        // The hex field is scaled 1.05 about its centre (its localScale); its text is the 24 scaled too.
        const FIELD_SCALE: f32 = 1.05;
        let field_at = in_picker("InputField (TMP)");
        let scaled_field = [field_at[0] - (FIELD_SCALE - 1.0) * field_at[2] / 2.0, field_at[1] - (FIELD_SCALE - 1.0) * field_at[3] / 2.0, field_at[2] * FIELD_SCALE, field_at[3] * FIELD_SCALE];
        let field_size = scene::font_size("Main", &format!("{FIELD}/Text Area/Text")) * FIELD_SCALE;
        let holder_pad = scene::padding("Main", &format!("{CP}/Button holder"), "HorizontalLayoutGroup")[0];
        let holder_gap = scene::spacing("Main", &format!("{CP}/Button holder"), "HorizontalLayoutGroup");
        let fill = || Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() };
        root.spawn(fill_centered()).with_children(|c| {
            c.spawn((Node { width: px(boxr[0]), height: px(boxr[1]), ..default() }, BackgroundColor(BLACK), dialog_outline())).with_children(|b| {
                b.spawn((Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..at_r(in_box("Title")) }, Pickable::IGNORE)).with_children(|t| tmp_bold(t, crate::il!(str "CathodeLightBehaviour::<Start>b__13_1" 0), 32.0, WHITE));
                // The Message's left edge as measured in the game's screenshot (26 in).
                b.spawn((Node { padding: UiRect::left(px(26)), ..at_r(in_box("Message")) }, Pickable::IGNORE)).with_children(|t| label(t, cb.description, 28.0, WHITE));
                b.spawn(at_r(picker)).with_children(|p| {
                    p.spawn((at_r(in_picker("outline")), BackgroundColor(scene::image_color("Main", &format!("{PICKER}/outline"))), Pickable::IGNORE));
                    // ColorpickerDialogBehaviour.UpdateElements: the handles at (HSV.y, 1 - HSV.z) of the box and 1 - HSV.x of the hue
                    // box, SaturationValueImage's _Hue = HSV.x, PreviewBox.color = RGB.
                    p.spawn((at_r(sv_box), super::loading::SaturationValueImage(h), ColourArea::SaturationValue, Hovered::default(), RelativeCursorPosition::default())).with_children(|sv| {
                        // Draggable: a 24 x 3 bar and its child turned 90 degrees, a cross.
                        let (x, y) = (side * s, side * (1.0 - v));
                        sv.spawn((at(x - 12.0, y - 1.5, 24.0, 3.0), BackgroundColor(WHITE), Pickable::IGNORE));
                        sv.spawn((at(x - 1.5, y - 12.0, 3.0, 24.0), BackgroundColor(WHITE), Pickable::IGNORE));
                    });
                    let stops = (0..=6).map(|k| srgb(crate::game::menu::hsv_to_rgb([1.0 - k as f32 / 6.0, 1.0, 1.0])).into()).collect();
                    p.spawn((at_r(hue_box), linear(LinearGradient::TO_BOTTOM, stops), ColourArea::Hue, Hovered::default(), RelativeCursorPosition::default())).with_children(|hb| {
                        hb.spawn((Node { position_type: PositionType::Absolute, left: px(0), right: px(0), top: px(hue_box[3] * (1.0 - h) - 2.0), height: px(4), ..default() }, BackgroundColor(WHITE), Pickable::IGNORE));
                    });
                    p.spawn((at_r(in_picker("preview box")), BackgroundColor(srgb(rgb)), Pickable::IGNORE));
                    p.spawn((Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..at_r(scaled_field) }, Tint::of_graphic("Main", FIELD, "TMP_InputField", &format!("{FIELD}/Text Area")).bundle(), Hovered::default(), Press(Act::ColourHex))).with_children(|f| {
                        label(f, cb.hex.clone(), field_size, WHITE);
                        if cb.editing {
                            label(f, "|", field_size, Color::srgb(0.19607843, 0.19607843, 0.19607843));
                        }
                    });
                });
                b.spawn(Node { padding: UiRect::all(px(holder_pad)), column_gap: px(holder_gap), ..at_r(in_box("Button holder")) }).with_children(|r| {
                    for (text, act) in [(cancel(), Act::ColourCancel), (apply(), Act::ColourApply)] {
                        r.spawn((Node { flex_grow: 1.0, flex_basis: px(0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, Tint::of("Main", &format!("{CP}/Button holder/Apply")).bundle(), Hovered::default(), Press(act))).with_children(|t| tmp_bold(t, text, 32.0, WHITE));
                    }
                });
            });
        });
    }
    if let Some(at) = ui.context {
        // Scrolling context menu: its top left pivot at the (window) point, ContextMenuBehaviour.Update's localScale 1.8; its
        // Content is a VerticalLayoutGroup (padding and spacing 2, upper left, children at their own sizes).
        const S: f32 = CONTEXT_SCALE;
        let k = ui.screen[0] / 2688.0;
        // The ScrollRect's fixed sizeDelta (160.78, 314.42); its Viewport is masked and Content (150 wide, ContentSizeFitter
        // preferred height) scrolls in it, clamped, with the AutoHideAndExpandViewport Scrollbar Vertical.
        let size = scene::rect("Main", CONTEXT_MENU_SCENE).size_delta;
        let content = scene::rect("Main", &format!("{CONTEXT_MENU_SCENE}/Viewport/Content")).size_delta[0];
        let menu = Node { position_type: PositionType::Absolute, left: px(at[0] / k), top: px(at[1] / k), width: px(size[0] * S), height: px(size[1] * S), ..default() };
        root.spawn((menu, BackgroundColor(scene::image_color("Main", CONTEXT_MENU_SCENE)))).with_children(|r| scroll_area(r, ui, CONTEXT_SCROLL, Node { flex_grow: 1.0, flex_direction: FlexDirection::Column, overflow: Overflow::scroll_y(), ..default() }, |v| {
            v.spawn(Node { width: px(content * S), flex_shrink: 0.0, padding: UiRect::all(px(2.0 * S)), align_items: AlignItems::FlexStart, flex_direction: FlexDirection::Column, row_gap: px(2.0 * S), ..default() }).with_children(|m| {
                let wide_node = || Node { width: px(150.0 * S), height: px(25.0 * S), flex_shrink: 0.0, align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default() };
                let last = CONTEXT_MENU.len() - 1;
                for (k, &(name, _)) in CONTEXT_MENU.iter().enumerate() {
                    let (text, tip) = if name == "-" { (String::new(), String::new()) } else { (scene::label("Main", &format!("{CONTEXT_MENU_PATH}/{name}/Text")), scene::tooltip("Main", &format!("{CONTEXT_MENU_PATH}/{name}"))) };
                    if name == "-" {
                        // The last one is the Separator Show turns on only above dynamic buttons.
                        if k != last || !ui.context_buttons.is_empty() {
                            m.spawn((Node { height: px(5.0 * S), width: px(150.0 * S), flex_shrink: 0.0, ..default() }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.16862746))));
                        }
                    } else if crate::game::Game::context_static_rewritten(name) {
                        // ContextMenuInteractableConditionalBehaviour.Update: button.interactable = flag, text.color = flag ? white :
                        // (1, 1, 1, 0.2) and tooltip.enabled = flag (no tooltip while off).
                        let on = ui.context_enabled.get(k).copied().unwrap_or(false);
                        let text = ui.context_labels.get(k).copied().flatten().map_or(text, str::to_string);
                        let mut e = m.spawn((wide_node(), BackgroundColor(CLEAR), Hovered::default(), Look { sel: Sel::None, on }));
                        if on {
                            e.insert((Press(Act::MenuStatic(k)), Tip(format!("{text}
{tip}"), false)));
                        }
                        e.with_children(|c| label(c, text.as_str(), 14.0 * S, if on { WHITE } else { Color::srgba(1.0, 1.0, 1.0, 0.2) }));
                    } else {
                        text_button(m, &text, 14.0 * S, None, Sel::None, &format!("{text}\n{tip} ({NOT_YET})"), wide_node());
                    }
                }
                for (i, b) in ui.context_buttons.iter().enumerate() {
                    let ready = !b.actions.iter().any(|a| a.needs_ui());
                    let tip = if ready { format!("{}\n{}", b.label, b.Description) } else { format!("{}\n{} ({NOT_YET})", b.label, b.Description) };
                    // The ContextMenuButton prefab: 136.6 by 25.58, text 14.
                    text_button(m, &b.label, 14.0 * S, ready.then_some(Act::Menu(i)), Sel::None, &tip, Node { width: px(136.6 * S), height: px(25.58 * S), ..wide_node() });
                }
            });
        }));
    }
}

/// The Status bar (Main scene): a row anchored top right (padding right 15, top 15, spacing 7.5, upper right) of fixed-size
/// items, each an Image black at 0.3137255; StatusController.Update sets which are active. In order: Tool Status and Toy
/// Status (48 squares), Follow Status, the limb status view, Thermal vision, Invis.Tools, Timescale Status, Framerate
/// Status. Each labelled item has its Status icon (48 high, preserve aspect, at its x) and its 18 text centred in the
/// rest (left 48).
fn status_bar(root: &mut Kids, ui: &Ui, art: &UiArt) {
    const BAR: &str = "Canvas/Status bar";
    let back = scene::image_color("Main", "Canvas/Status bar/Follow Status");
    let bar_pad = scene::padding("Main", BAR, "HorizontalLayoutGroup");
    // The Status bar's localScale 1.3 about its pivot, the top right corner.
    const K: f32 = 1.3;
    root.spawn((Node { position_type: PositionType::Absolute, right: px(0), top: px(0), height: px(scene::rect("Main", BAR).size_delta[1] * K), justify_content: JustifyContent::FlexEnd, align_items: AlignItems::FlexStart, padding: UiRect { right: px(bar_pad[1] * K), top: px(bar_pad[2] * K), ..default() }, ..row(scene::spacing("Main", BAR, "HorizontalLayoutGroup") * K) }, Pickable::IGNORE)).with_children(|bar| {
        let square = |bar: &mut Kids, a: Option<&Art>| {
            bar.spawn((Node { width: px(48.0 * K), height: px(48.0 * K), flex_shrink: 0.0, ..default() }, BackgroundColor(back), Pickable::IGNORE)).with_children(|s| {
                if let Some(a) = a {
                    picture(s, a, 48.0 * K, 48.0 * K, WHITE);
                }
            });
        };
        // ToolStatusView.Start: Image.sprite = GetByType(CurrentTool) when there is one.
        // ToolStatusView.ToolChanged: the same on every tool change (ToolControllerBehaviour.add_ToolChanged: the bar is drawn from `tool`).
        // ToolStatusView.OnDestroy: unsubscribes (ToolControllerBehaviour.remove_ToolChanged: nothing is subscribed here).
        // CurrentTool is always set, so the square is always active.
        square(bar, tool_status_icon(&ui.tool, art));
        // ToyStatusView.Start: subscribes to SelectionChanged and UpdateSprite.
        // ToyStatusView.Catalog_SelectionChanged: active while the catalog has a selected item (contraptions are not rewritten).
        // ToyStatusView.UpdateSprite: Image.sprite = SelectedItem.ViewSprite.
        if let Some(i) = &ui.item {
            square(bar, art.items.get(i));
        }
        // An item: width, the icon's x and sprite, its text (static or bound), the item it is.
        let item = |bar: &mut Kids, name: &str, icon: &str, text: Option<&str>, text_of: Option<Bound>, shown: Option<StatusItem>| {
            let at = format!("{BAR}/{name}");
            let (width, icon_x) = (scene::rect("Main", &at).size_delta[0], scene::rect("Main", &format!("{at}/Status icon")).anchored_position[0]);
            // StatusController.Update shows these from the first frame on.
            let display = if shown.is_some() { Display::None } else { Display::Flex };
            let mut e = bar.spawn((Node { width: px(width * K), height: px(48.0 * K), flex_shrink: 0.0, display, ..default() }, BackgroundColor(back), Pickable::IGNORE));
            if let Some(s) = shown {
                e.insert(s);
            }
            e.with_children(|d| {
                d.spawn(Node { position_type: PositionType::Absolute, left: px(icon_x * K), top: px(0), width: px(48.0 * K), height: px(48.0 * K), ..default() }).with_children(|i| {
                    if let Some(a) = art.named.get(icon) {
                        picture(i, a, 48.0 * K, 48.0 * K, WHITE);
                    }
                });
                d.spawn((Node { position_type: PositionType::Absolute, left: px(48.0 * K), right: px(0), top: px(0), bottom: px(0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|c| {
                    if let Some(s) = text {
                        label(c, s, 18.0 * K, WHITE);
                    }
                    match text_of {
                        // Framerate: fontStyle SmallCaps, so " fps" is drawn "FPS" at 0.8 of the size.
                        Some(Bound::Framerate) => {
                            c.spawn((Text::new("000"), font(18.0 * K), TextColor(WHITE), Bound::Framerate, Pickable::IGNORE)).with_children(|t| {
                                t.spawn((TextSpan::new(" FPS"), font(18.0 * K * 0.8), TextColor(WHITE)));
                            });
                        }
                        Some(b) => bound(c, b, 18.0 * K, WHITE),
                        None => {}
                    }
                });
            });
        };
        // FollowingViewBehaviour's Follow Status: active while CurrentlyFollowing is not empty (CameraControlBehaviour.LateUpdate).
        if let Some(t) = &ui.follow {
            item(bar, "Follow Status", "crosshair", Some(t), None, None);
        }
        // LimbStatusView: active while Global.ShowLimbStatus.
        if ui.detail {
            item(bar, "Limb status view.. status", "Details", Some(&scene::label("Main", "Canvas/Status bar/Limb status view.. status/TextMeshPro Text")), None, None);
        }
        // StatusController.Update: Thermal vision while ThermalVisionEnabled, Invis.Tools while the tools are invisible.
        if ui.thermal {
            item(bar, "Themal vision", "ThermalVision", Some(&scene::label("Main", "Canvas/Status bar/Themal vision/TextMeshPro Text")), None, None);
        }
        if ui.tools_invisible {
            item(bar, "Invis.Tools", "eye_closed", Some(&scene::label("Main", "Canvas/Status bar/Invis.Tools/TextMeshPro Text")), None, None);
        }
        item(bar, "Timescale Status", "Time", None, Some(Bound::TimeScale), Some(StatusItem::TimeScale));
        item(bar, "Framerate Status", "Framerate", None, Some(Bound::Framerate), Some(StatusItem::Framerate));
    });
}

/// ToolStatusView.GetByType: the icon of the first Tools entry, else the first Powers entry, whose Type is the chosen tool's.
fn tool_status_icon<'a>(tool: &str, art: &'a UiArt) -> Option<&'a Art> {
    let all = || art.tools_list.iter().chain(&art.powers_list);
    let ty = &all().find(|t| t.name == tool)?.ty;
    let first = all().find(|t| &t.ty == ty)?;
    art.tools.get(&first.name)
}

/// The Unity version of the game's data (the globalgamemanagers header: Application.unityVersion).
const UNITY_VERSION: &str = "2020.3.1f1";

/// The Information page's objects in the Menu scene: the layout, and its fixed-height texts.
const INFO: &str = "Canvas/Information/Info";
const INFO_TEXT: &str = "Canvas/Information/Info/info";
const INFO_VERSION: &str = "Canvas/Information/Info/version info";
const INFO_LINKS: &str = "Canvas/Information/Info/version info (1)";

/// The CanvasScaler's reference width: every size is in these units.
pub const CANVAS_WIDTH: f32 = 2688.0;

/// The Menu scene path of the Open-source licenses page's `info`.
const LICENSES_PATH: &str = "Canvas/Open-source licenses/info container/viewport/content/info";

/// The Information page's buttons: a row (the Info/discord row's height) holding the Button, which stretches over the Info width less
/// the Button's negative size delta; its Image and Button tints and a bold 28 title.
fn info_button(p: &mut Kids, text: &str, act: Act) {
    const BUTTON: &str = "Canvas/Information/Info/button/Button";
    // The slot ("button"/"lic") of its authored height; the Button stretched over it but for its left offset.
    let b = scene::rect("Menu", BUTTON);
    let (left, right) = (b.anchored_position[0] - b.size_delta[0] * b.pivot[0], b.anchored_position[0] + b.size_delta[0] * (1.0 - b.pivot[0]));
    p.spawn(Node { height: px(scene::rect("Menu", "Canvas/Information/Info/button").size_delta[1]), flex_shrink: 0.0, ..row(0.0) }).with_children(|r| {
        r.spawn((Node { flex_grow: 1.0, height: percent(100), margin: UiRect { left: px(left), right: px(-right), ..default() }, justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, Tint::of("Menu", BUTTON).bundle(), Hovered::default(), Press(act))).with_children(|t| tmp_bold(t, text, 28.0, WHITE));
    });
}

/// One TextMesh Pro rich text style: <b>, <u>, <size=N%>, <color> and <line-height=N%>.
#[derive(Clone, Copy, PartialEq)]
struct Style {
    bold: bool,
    underline: bool,
    italic: bool,
    color: Color,
    size: f32,
    line: f32,
}

/// TMP's colour value: #rgb, #rgba, #rrggbb, #rrggbbaa, or a name.
fn tmp_color(v: &str) -> Option<Color> {
    let v = v.trim_matches('"');
    let Some(h) = v.strip_prefix('#') else {
        return match v {
            "red" => Some(Color::srgb(1.0, 0.0, 0.0)),
            "white" => Some(WHITE),
            "black" => Some(BLACK),
            "green" => Some(Color::srgb(0.0, 1.0, 0.0)),
            "blue" => Some(Color::srgb(0.0, 0.0, 1.0)),
            "yellow" => Some(Color::srgb(1.0, 0.92156863, 0.015686275)),
            "orange" => Some(Color::srgb(1.0, 0.5019608, 0.0)),
            _ => None,
        };
    };
    let d = |s: &str| u8::from_str_radix(s, 16).ok().map(|x| x as f32 / 255.0);
    let pair = |k: usize| -> Option<f32> {
        if h.len() <= 4 { d(&h.get(k / 2..k / 2 + 1)?.repeat(2)) } else { d(h.get(k..k + 2)?) }
    };
    match h.len() {
        3 | 6 => Some(Color::srgb(pair(0)?, pair(2)?, pair(4)?)),
        4 | 8 => Some(Color::srgba(pair(0)?, pair(2)?, pair(4)?, pair(6)?)),
        _ => None,
    }
}

/// ContextMenuBehaviour.Update's localScale.
pub const CONTEXT_SCALE: f32 = 1.8;
const CONTEXT_MENU_SCENE: &str = "Canvas/Scrolling context menu";
/// The context menu's ScrollRect, as a scroll view id.
const CONTEXT_SCROLL: u8 = 12;

/// The DialogBox prefab text's font asset's m_FaceInfo (arial SDF; LiberationSans SDF has the same metrics) over its
/// point size: line height, ascent line, descent line (negative). Set from the game's asset when the UI art loads.
pub static TMP_FACE: std::sync::OnceLock<[f32; 3]> = std::sync::OnceLock::new();

/// The DialogBox prefab's "Text (TMP)" TextMeshProUGUI (its font, size 36, characterSpacing -2.32, word wrapping, the rect
/// it has before the layout runs). Set from the game's asset when the UI art loads.
pub static DIALOG_TMP: std::sync::OnceLock<crate::tmp::TextProps> = std::sync::OnceLock::new();

/// TMP_Text.GetPreferredValues() of the dialog box text after `text = text` (tmp::layout); zero without the game's asset.
pub fn dialog_preferred_values(text: &str) -> [f32; 2] {
    DIALOG_TMP.get().map_or([0.0; 2], |p| crate::tmp::layout::preferred_values(&crate::tmp::TextProps { text: text.into(), text_input_box: false, ..p.clone() }))
}

/// TMP's line pitch for the default line height (1.2 until the font asset is read, as in unit tests without the game).
fn tmp_line() -> f32 {
    TMP_FACE.get().map_or(1.2, |f| f[0])
}

/// TMP_Text.preferredHeight of an unwrapped rich text: from the first line's top (its tallest ascender) to the last
/// line's bottom (its lowest descender); each line break adds the font's line gap (line height - ascent + descent) at the
/// size in effect there. A last line break with nothing after it adds nothing.
pub fn tmp_preferred_height(text: &str, size: f32) -> f32 {
    let [line, ascent, descent] = TMP_FACE.get().copied().unwrap_or([1.2, 1.0, -0.2]);
    let lines = rich_lines(&text.replace("\\n", "\n").replace('\r', ""), size);
    let tallest = |l: &Vec<(String, Style)>| l.iter().map(|r| r.1.size).fold(0.0, f32::max).max(if l.is_empty() { size } else { 0.0 });
    let at_break = |l: &Vec<(String, Style)>| l.last().map_or(size, |r| r.1.size);
    let body: f32 = lines.iter().map(|l| (ascent - descent) * tallest(l)).sum();
    let gaps: f32 = lines.iter().rev().skip(1).map(|l| (line - ascent + descent) * at_break(l)).sum();
    body + gaps
}

/// TextMesh Pro rich text as lines of (text, style) runs; a tag TMP does not know is text. The styles carry over a newline.
fn rich_lines(text: &str, size: f32) -> Vec<Vec<(String, Style)>> {
    let base = Style { bold: false, underline: false, italic: false, color: WHITE, size, line: tmp_line() };
    let (mut stack, mut cur): (Vec<Style>, Style) = (Vec::new(), base);
    let (mut lines, mut line, mut run) = (Vec::new(), Vec::new(), String::new());
    let flush = |line: &mut Vec<(String, Style)>, run: &mut String, s: Style| {
        if !run.is_empty() {
            line.push((std::mem::take(run), s));
        }
    };
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '\n' {
            flush(&mut line, &mut run, cur);
            lines.push(std::mem::take(&mut line));
            rest = &rest[1..];
            continue;
        }
        if c == '<' {
            if let Some(j) = rest.find('>') {
                let tag = &rest[1..j];
                let percent = |name: &str| tag.strip_prefix(name).and_then(|v| v.strip_suffix('%')).and_then(|v| v.parse::<f32>().ok());
                let mut next = cur;
                let mut push = true;
                let known = if tag == "b" {
                    next.bold = true;
                    true
                } else if tag == "u" {
                    next.underline = true;
                    true
                } else if tag == "i" {
                    next.italic = true;
                    true
                } else if matches!(tag, "/b" | "/u" | "/i" | "/size" | "/color" | "/line-height") {
                    next = stack.pop().unwrap_or(base);
                    push = false;
                    true
                } else if let Some(v) = percent("size=") {
                    next.size = size * v / 100.0;
                    true
                } else if let Some(v) = percent("line-height=") {
                    next.line = tmp_line() * v / 100.0;
                    true
                } else if let Some(col) = tag.strip_prefix("color=").and_then(tmp_color) {
                    next.color = col;
                    true
                } else if let Some(a) = tag.strip_prefix("alpha=#").and_then(|v| u8::from_str_radix(v, 16).ok()) {
                    // <alpha=#XX>: the opacity from here on (not a scope: no closing tag pops it)
                    next.color = next.color.with_alpha(a as f32 / 255.0);
                    push = false;
                    true
                } else {
                    false
                };
                if known {
                    flush(&mut line, &mut run, cur);
                    if push {
                        stack.push(cur);
                    }
                    cur = next;
                    rest = &rest[j + 1..];
                    continue;
                }
            }
        }
        run.push(c);
        rest = &rest[c.len_utf8()..];
    }
    flush(&mut line, &mut run, cur);
    lines.push(line);
    // A last line break with nothing after it adds no line (TMP's preferred height ends at the last character).
    while lines.len() > 1 && lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

/// The italic runs of a text as (text, bold, size, line height), for italic.rs to rasterise.
pub fn italic_runs(text: &str, size: f32) -> Vec<(String, bool, f32, f32)> {
    rich_lines(text, size).into_iter().flatten().filter(|(_, s)| s.italic).map(|(t, s)| (t, s.bold, s.size, s.line)).collect()
}

/// Rich text as a column of lines: a line of one style wraps as one text (the paragraphs), a line of several styles is its runs side by side;
/// an empty line is one line high. Bold thickens the glyphs (the fonts have no bold face), <u> underlines.
fn rich(p: &mut Kids, text: &str, size: f32, justify: Justify) {
    let align = match justify {
        Justify::Right => JustifyContent::FlexEnd,
        Justify::Center => JustifyContent::Center,
        _ => JustifyContent::FlexStart,
    };
    let run = |e: &mut Kids, s: &str, st: Style, layout: TextLayout| {
        // <i>: the run rasterised with the font asset's shear (italic.rs); one unrasterised (no such text in the scenes) is drawn upright.
        if st.italic {
            if let Some((image, size, margin)) = super::italic::run(s, st.bold, st.size, st.line) {
                e.spawn((Node { width: px(size[0] - 2.0 * margin), height: px(size[1]), flex_shrink: 0.0, ..default() }, Pickable::IGNORE)).with_children(|i| {
                    i.spawn((Node { position_type: PositionType::Absolute, left: px(-margin), top: px(0), width: px(size[0]), height: px(size[1]), ..default() }, ImageNode { image, color: st.color, ..default() }, Pickable::IGNORE, ShownText(s.to_string())));
                });
                return;
            }
        }
        let mut t = e.spawn((Text::new(s), font(st.size), bevy::text::LineHeight::RelativeToFont(st.line), TextColor(st.color), layout, Pickable::IGNORE));
        if st.bold {
            t.insert(TextShadow { offset: Vec2::new(1.2, 0.0), color: st.color });
        }
        if st.underline {
            t.insert(bevy::text::Underline);
        }
    };
    // TMP_Text.parseCtrlCharacters: a backslash and n or t in the text are a line break and a tab.
    // A carriage return (Environment.NewLine's \r) takes no room and breaks no line.
    for line in rich_lines(&text.replace("\\n", "\n").replace("\\t", "\t").replace('\r', ""), size) {
        match line.as_slice() {
            [] => run(p, " ", Style { bold: false, underline: false, italic: false, color: WHITE, size, line: tmp_line() }, TextLayout::default()),
            [(s, st)] => run(p, s, *st, TextLayout { justify, ..default() }),
            runs => {
                p.spawn((Node { justify_content: align, align_items: AlignItems::FlexEnd, ..row(0.0) }, RichLine)).with_children(|r| {
                    for (s, st) in runs {
                        run(r, s, *st, TextLayout::default().with_no_wrap());
                    }
                });
            }
        }
    }
}

/// ItemButton.prefab's "button row": a HorizontalLayoutGroup along the button's bottom (children 27 x 27, alignment lower centre) holding ViewOnSteam,
/// Publish and View info, each active per ItemButtonBehaviour.Start.
fn item_buttons(cell: &mut Kids, start: &super::catalog::ItemButtonStart, index: usize, art: &UiArt) {
    const ROW: &str = "button row";
    let (row_rect, size) = (scene::rect("ItemButton", ROW), scene::rect("ItemButton", "button row/View info").size_delta);
    let group = |f: &str| scene::number("ItemButton", ROW, "HorizontalLayoutGroup", f);
    let pad = scene::padding("ItemButton", ROW, "HorizontalLayoutGroup");
    cell.spawn((
        Node { position_type: PositionType::Absolute, left: px(0), right: px(0), bottom: px(0), height: px(row_rect.size_delta[1]), padding: UiRect { left: px(pad[0]), right: px(pad[1]), top: px(pad[2]), bottom: px(pad[3]) }, column_gap: px(group("m_Spacing")), justify_content: JustifyContent::Center, align_items: AlignItems::End, ..default() },
        Pickable::IGNORE,
    ))
    .with_children(|r| {
        for (name, shown, icon, act) in [("ViewOnSteam", start.workshop, "ItemWorkshop", Some(Act::ItemWorkshop(index))), ("Publish", start.publish, "ItemPublish", Some(Act::ItemPublish(index))), ("View info", start.info, "ItemInfo", Some(Act::ItemInfo(index)))] {
            if !shown {
                continue;
            }
            let path = format!("{ROW}/{name}");
            let mut e = r.spawn(Node { width: px(size[0]), height: px(size[1]), flex_shrink: 0.0, ..default() });
            if let (Some(a), Some(act)) = (art.named.get(icon), act) {
                e.insert((Hovered::default(), Press(act), Tip(scene::tooltip("ItemButton", &path), false)));
                e.with_children(|b| picture(b, a, size[0], size[1], scene::image_color("ItemButton", &path)));
            }
        }
    });
}

const CAT: &str = "Canvas/Catalog";

/// LayoutGroup.SetChildrenAlongAxis on a group's own axis below its children's total preferred size: each child takes
/// min..preferred at InverseLerp(total min, total preferred, size). `children` are each one's (min, preferred - min).
fn group_heights<const N: usize>(size: f32, children: &[(f32, f32); N]) -> [f32; N] {
    let (min, pref) = children.iter().fold((0.0, 0.0), |(a, b), (m, p)| (a + m, b + m + p));
    let t = if pref == min { 0.0 } else { ((size - min) / (pref - min)).clamp(0.0, 1.0) };
    children.map(|(m, p)| m + p * t)
}

/// A HorizontalLayoutGroup's children widths as drawn (childScale: each counts at its scale, flexible too): min 0, preferred 100 (the
/// Square sprite), `fixed` the padding and spacing. Above the total preferred the rest goes by flexible width to the
/// unscaled sizes (SetChildrenAlongAxis), each then drawn at its scale; below, min..preferred.
fn group_widths(size: f32, fixed: f32, children: &[(f32, f32)]) -> Vec<f32> {
    let pref: f32 = fixed + children.iter().map(|(s, _)| 100.0 * s).sum::<f32>();
    let flex: f32 = children.iter().map(|(s, f)| f.max(0.0) * s).sum();
    if size >= pref {
        let m = if flex > 0.0 { (size - pref) / flex } else { 0.0 };
        children.iter().map(|(s, f)| (100.0 + f.max(0.0) * m) * s).collect()
    } else {
        let t = ((size - fixed) / (pref - fixed)).max(0.0);
        children.iter().map(|(s, _)| 100.0 * t * s).collect()
    }
}

fn catalog(root: &mut Kids, ui: &Ui, art: &UiArt, width: f32) {
    if ui.catalog.categories.len() > 9 {
        unimplemented!("mod-only: CatalogDropdownBehaviour.Drop: the category dropdown for more than 9 categories, in {} containers", super::catalog::category_containers(ui.catalog.categories.len()).len());
    }
    let tile = scene::vec2("Main", &format!("{CAT}/ItemPanel"), "GridLayoutGroup", "m_CellSize")[0];
    let gap = scene::vec2("Main", &format!("{CAT}/ItemPanel"), "GridLayoutGroup", "m_Spacing");
    let delete = scene::rect("Main", &format!("{CAT}/CategoryContainer/Filter/reset")).size_delta;
    // ResizableUIElementHandle (Main scene): Graphic black at 0.329 alpha (Default), anchored to the panel's right edge, 14 wide.
    root.spawn((Node { position_type: PositionType::Absolute, left: px(width), top: px(0), bottom: px(0), width: px(scene::rect("Main", HANDLE).size_delta[0]), ..default() }, BackgroundColor(handle_color("Default")), Hovered::default(), ResizeHandle));
    root.spawn((Node { position_type: PositionType::Absolute, left: px(0), top: px(0), bottom: px(0), width: px(width), ..column(0.0) }, BackgroundColor(scene::image_color("Main", CAT)))).with_children(|c| {
        // MainCategoryPanel: the icons share its width when they do not fit.
        let size = 40f32.min(width / ui.catalog.categories.len().max(1) as f32);
        c.spawn((Node { height: px(scene::rect("Main", &format!("{CAT}/CategoryContainer/MainCategoryPanel")).size_delta[1]), flex_shrink: 0.0, justify_content: JustifyContent::Center, ..row(0.0) }, BackgroundColor(BLACK))).with_children(|cats| {
            let made = ui.catalog.categories.iter().filter(|c| c.icon.is_some()).count();
            super::uifn("CatalogBehaviour.CreateCategoryButtons", || (scene::children("Main", &format!("{CAT}/CategoryContainer/MainCategoryPanel")).len() + made).to_string());
            // CategoryButton.prefab: the Image is the icon (CreateCategoryButtons sets its sprite) and the Button's target graphic.
            let category_tint = Tint::of("CategoryButton", "");
            for (i, cat) in ui.catalog.categories.iter().enumerate() {
                cats.spawn((Node { width: px(size), height: px(size), ..default() }, BackgroundColor(CLEAR), Hovered::default(), category_tint, TintPicture, Press(Act::Category(i)), Tip(tooltip(&cat.name, &cat.description), false))).with_children(|b| {
                    if let Some(Some(a)) = art.categories.get(i) {
                        picture(b, a, size, size, WHITE);
                    }
                });
            }
        });
        // Filter.
        // The Input field stretched over the row but for its offsets (its text 8 in), the reset button middle right.
        let reset = scene::rect("Main", &format!("{CAT}/CategoryContainer/Filter/reset"));
        c.spawn(Node { height: px(scene::rect("Main", &format!("{CAT}/CategoryContainer/Filter")).size_delta[1]), flex_shrink: 0.0, ..default() }).with_children(|f| {
            f.spawn((Node { padding: UiRect::left(px(8)), align_items: AlignItems::Center, ..stretched(&scene::rect("Main", &format!("{CAT}/CategoryContainer/Filter/Input field"))) }, Hovered::default(), Press(Act::Filter))).with_children(|i| bound(i, Bound::Filter, 22.0, WHITE));
            f.spawn((Node { position_type: PositionType::Absolute, right: px(-reset.anchored_position[0]), top: percent(50), margin: UiRect::top(px(-delete[1] / 2.0)), width: px(delete[0]), height: px(delete[1]), ..default() }, Hovered::default(), Press(Act::ClearFilter))).with_children(|r| {
                if let Some(a) = art.named.get("Delete") {
                    picture(r, a, delete[0], delete[1], WHITE);
                }
            });
        });
        // ItemPanel.
        scroll_area(c, ui, 3, scroll_node(), |s| {
            // ItemButton.prefab: the Image (ItemButtonBehaviour.Start's ViewSprite) is the Button's target graphic.
            let item_tint = Tint::of("ItemButton", "").with_selected("ItemButton", "");
            s.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(gap[0]), row_gap: px(gap[1]), padding: UiRect::all(px(14)), flex_shrink: 0.0, ..default() }).with_children(|g| {
                for (index, item) in ui.catalog.shown(ui.category, ui.list_filter()) {
                    // ItemButtonBehaviour.Start's tooltip text.
                    let start = super::catalog::ItemButton::of(item).start();
                    let tip = tooltip(&item.name, start.tooltip.split_once(char::from(10)).map_or("", |(_, d)| d));
                    let act = if item.local.is_some() { Act::Contraption(index) } else { Act::Item(item.name.clone()) };
                    g.spawn(Node { width: px(tile), height: px(tile), flex_shrink: 0.0, ..default() }).with_children(|cell| {
                        cell.spawn((Node { position_type: PositionType::Absolute, left: px(0), top: px(0), width: px(tile), height: px(tile), ..default() }, BackgroundColor(CLEAR), Hovered::default(), item_tint, TintPicture, Press(act), Tip(tip, false))).with_children(|b| {
                            match art.items.get(&super::art::item_key(item)) {
                                Some(a) => picture(b, a, tile, tile, WHITE),
                                // The rewrite's stand-in for a missing thumbnail (the game's name text is alpha 0).
                                None => {
                                    b.spawn((Node::default(), NotCompared("no thumbnail"))).with_children(|n| label(n, item.name.clone(), 18.0, WHITE));
                                }
                            }
                        });
                        item_buttons(cell, &start, index, art);
                        // ItemButtonBehaviour.Start: the contraption's remove button (DeleteContraptionButtonBehaviour) at the corner.
                        if item.local.is_some() {
                            cell.spawn((Node { position_type: PositionType::Absolute, right: px(4), top: px(4), width: px(delete[0]), height: px(delete[1]), ..default() }, Hovered::default(), Press(Act::DeleteContraption(index)))).with_children(|r| {
                                if let Some(a) = art.named.get("Delete") {
                                    picture(r, a, delete[0], delete[1], WHITE);
                                }
                            });
                        }
                    });
                }
            });
        });
        // settings: a VerticalLayoutGroup of the rows "map settings" and "clear" (HorizontalLayoutGroups): all control and
        // force expand with childScale, so a child counts at its localScale (Settings and Environment 0.952). Each
        // button's preferred size is its Square sprite's (4 px at 4 pixels per unit: 100), min 0, flexible width its
        // LayoutElement's. Main.unity's TooltipBehaviour texts.
        let set = format!("{CAT}/settings");
        let rows: [(&str, &[(&str, &str, Act, Sel, &str)]); 2] = [
            ("map settings", &[
                ("Settings", "Settings", Act::Nav(Page::Settings), Sel::None, "Options menu."),
                ("Environment", "Environment", Act::Environment(true), Sel::Env, "Access environment settings. These will only last for the current session."),
            ]),
            ("clear", &[
                ("Clear Everything", "Clear everything", Act::ClearEverything, Sel::None, "Clears everything."),
                ("Clear Mess", "", Act::ClearMess, Sel::None, "Clears all debris and decals."),
                ("Clear Living", "Clear living", Act::ClearLiving, Sel::None, "Clears all \"living\" things."),
            ]),
        ];
        let pads = rows.map(|(row, _)| scene::padding("Main", &format!("{set}/{row}"), "HorizontalLayoutGroup"));
        let scale = |row: &str, b: &str| scene::rect("Main", &format!("{set}/{row}/{b}")).scale;
        // The rows' min is their vertical padding, preferred that plus their tallest scaled button.
        let heights = group_heights(
            scene::rect("Main", &set).size_delta[1],
            &std::array::from_fn::<_, 2, _>(|i| (pads[i][2] + pads[i][3], rows[i].1.iter().map(|b| scale(rows[i].0, b.0)[1] * 100.0).fold(0.0, f32::max))),
        );
        c.spawn(Node { height: px(scene::rect("Main", &set).size_delta[1]), flex_shrink: 0.0, ..column(0.0) }).with_children(|m| {
            for (i, (row, buttons)) in rows.iter().enumerate() {
                let path = format!("{set}/{row}");
                let [l, r, t, b] = pads[i];
                let spacing = scene::number("Main", &path, "HorizontalLayoutGroup", "m_Spacing");
                let along: Vec<(f32, f32)> = buttons.iter().map(|b| (scale(row, b.0)[0], scene::number("Main", &format!("{path}/{}", b.0), "LayoutElement", "m_FlexibleWidth"))).collect();
                let widths = group_widths(width, l + r + spacing * (along.len() - 1) as f32, &along);
                let inner = heights[i] - t - b;
                // Across: each button's unscaled height is the row's inner height, drawn at its scale from the top.
                m.spawn(Node { height: px(heights[i]), flex_shrink: 0.0, flex_direction: FlexDirection::Row, align_items: AlignItems::FlexStart, padding: UiRect { left: px(l), right: px(r), top: px(t), bottom: px(b) }, column_gap: px(spacing), ..default() }).with_children(|r| {
                    for ((name, text, act, sel, tip), w) in buttons.iter().zip(widths) {
                        let k = scale(row, name)[1];
                        let text = if text.is_empty() { scene::label("Main", &format!("{path}/{name}/TextMeshPro Text")) } else { text.to_string() };
                        r.spawn((Node { width: px(w), height: px(inner * k), flex_shrink: 0.0, padding: UiRect::all(px(5.0 * k)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, BackgroundColor(CLEAR), Hovered::default(), Look { sel: sel.clone(), on: true }, Tip(tip.to_string(), false), Press(act.clone()))).with_children(|c| {
                            // TMP auto-size (10 to 23): every label fits at 23 in the catalog width, so it never shrinks.
                            c.spawn((Text::new(text), font(23.0 * k), TextColor(WHITE), TextShadow { offset: Vec2::new(0.6 * k, 0.0), color: WHITE }, TextLayout { justify: Justify::Center, ..default() }, Pickable::IGNORE));
                        });
                    }
                });
            }
        });
        // Tooltip, over the bottom of the catalog.
        c.spawn((Node { position_type: PositionType::Absolute, left: px(0), right: px(0), bottom: px(0), height: px(scene::rect("Main", &format!("{CAT}/Tooltip")).size_delta[1]), padding: UiRect::all(px(16)), ..column(4.0) }, BackgroundColor(BLACK), Visibility::Hidden, TipBox(false), Pickable::IGNORE)).with_children(|t| {
            bound(t, Bound::Tip(false, true), 18.0, WHITE);
            bound(t, Bound::Tip(false, false), 18.0, WHITE);
        });
    });
}

/// ToolBarGenerator: an icon per parentless tool, its children in a black strip to the left while it is hovered.
const TOOLBAR: &str = "Canvas/Toolbar";

fn toolbar(canvas: &mut Kids, ui: &Ui, controls: &Controls, art: &UiArt) {
    // The Toolbar is centred on the right edge: its children count from the middle.
    let bar = scene::rect("Main", TOOLBAR);
    // ToolWheelBehaviour.Start and Toggle: localScale = Vector3.one * 1.2 about the Toolbar's pivot (1, 0.5), the zero-size
    // node on the canvas's right middle that the toolbar's pieces hang from.
    canvas.spawn((Node { position_type: PositionType::Absolute, right: px(0), top: percent(50), width: px(0), height: px(0), ..default() }, UiTransform { scale: Vec2::splat(1.2), ..default() }, Pickable::IGNORE)).with_children(|root| toolbar_wheel(root, ui, controls, art, bar));
    // ToolTooltip: right aligned, above the Toolbar.
    let tip = scene::rect("Main", "Canvas/ToolTooltip ");
    canvas.spawn((Node { position_type: PositionType::Absolute, right: px(-tip.anchored_position[0]), top: percent(50), margin: UiRect::top(px(-tip.anchored_position[1])), align_items: AlignItems::FlexEnd, ..column(2.0) }, Visibility::Hidden, TipBox(true), Pickable::IGNORE)).with_children(|t| {
        bound(t, Bound::Tip(true, true), 18.0, WHITE);
        bound(t, Bound::Tip(true, false), 18.0, WHITE);
    });
}

fn toolbar_wheel(root: &mut Kids, ui: &Ui, controls: &Controls, art: &UiArt, bar: crate::assets::classes::RectTransform) {
    let from_middle = |y: f32, height: f32| Node { position_type: PositionType::Absolute, right: px(0), width: px(bar.size_delta[0]), height: px(height), top: percent(50), margin: UiRect::top(px(-(y + height))), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() };
    for (name, powers, tab) in [(scene::label("Main", "Canvas/Catalog/ToolPanel/TextMeshPro Text").to_uppercase(), false, "tab-tools"), (scene::label("Main", "Canvas/Toolbar/tab-powers/TextMeshPro Text").to_uppercase(), true, "tab-powers")] {
        let tab = scene::rect("Main", &format!("{TOOLBAR}/{tab}"));
        root.spawn((from_middle(tab.anchored_position[1], tab.size_delta[1]), BackgroundColor(BLACK))).with_children(|t| {
            t.spawn((Node { width: percent(100), height: percent(100), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, BackgroundColor(CLEAR), Hovered::default(), Look { sel: Sel::Powers(powers), on: true }, Press(Act::Powers(powers)))).with_children(|b| label(b, name, 14.0, WHITE));
        });
    }
    root.spawn((Node { position_type: PositionType::Absolute, right: px(0), width: px(bar.size_delta[0]), height: px(bar.size_delta[1]), top: percent(50), margin: UiRect::top(px(-bar.size_delta[1] / 2.0)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..column(5.0) }, BackgroundColor(BLACK))).with_children(|bar| {
        let list = if ui.powers { &art.powers_list } else { &art.tools_list };
        // ToolBarGenerator.GenerateButtonsFor: CreateButton for each parentless entry.
        for tool in list.iter().filter(|t| t.parent.is_empty()) {
            tool_button(bar, &tool_button_spec(tool, list), art, None);
        }
        let _ = controls;
    });
    let eye = scene::rect("Main", &format!("{TOOLBAR}/toggle-visibility"));
    // ToolWheelBehaviour.SetToolVisibility: EyeImage.sprite = EyeClosed while the tools are invisible, else EyeOpen.
    root.spawn((from_middle(eye.anchored_position[1], eye.size_delta[1]), BackgroundColor(BLACK), Tip("Tool visibility\nToggle the visibility of tools".into(), true), Hovered::default(), Press(Act::ToolVisibility))).with_children(|t| {
        if let Some(a) = art.named.get(if ui.tools_invisible { "eye_closed" } else { "eye_open" }) {
            picture_with(t, a, bar.size_delta[0], eye.size_delta[1], WHITE, EyeImage);
        }
    });
}

/// ToolBarGenerator.CreateButton's state: the button's name, HasTooltipBehaviour.Text ("<b>Name</b>\nDescription"), its
/// onClick SetToolByTypeName(tool.Type) (every tool has one; a type no game tool has is a mod's, set_tool_by_type_name),
/// and the child container's buttons, made the same way (recursively) from every entry of the list whose Parent is this
/// Name, but the tool itself (Tool.op_Equality is Tool.Equals: the Types). A child's onClick also hides that container
/// (Flyouts::clicked_in); the button's hover shows and hides it (Flyouts::hover). The toolbar view and --fn-check
/// (fnbiga.rs, biga:toolbar) build from it.
pub struct ToolButton<'a> {
    pub tool: &'a ToolDef,
    pub text: String,
    pub children: Vec<ToolButton<'a>>,
}

pub fn tool_button_spec<'a>(tool: &'a ToolDef, all: &'a [ToolDef]) -> ToolButton<'a> {
    ToolButton {
        tool,
        text: format!("<b>{}</b>\n{}", tool.name, tool.description),
        children: all.iter().filter(|t| t.ty != tool.ty && t.parent == tool.name).map(|t| tool_button_spec(t, all)).collect(),
    }
}

impl Flyouts {
    /// UiHoverEventEmitter.OnMouseEnter / OnMouseExit of the button with this child container: SetActive(hovered) when
    /// the hover changes. Returns the container's activeSelf.
    pub fn hover(&mut self, name: &str, hovered: bool) -> bool {
        let (open, was) = self.0.entry(name.to_string()).or_default();
        if hovered != *was {
            *open = hovered;
        }
        *was = hovered;
        *open
    }
    /// A child button's second onClick listener: childContainer.SetActive(false).
    pub fn clicked_in(&mut self, parent: &str) {
        self.0.entry(parent.to_string()).or_default().0 = false;
    }
    pub fn open(&self, name: &str) -> bool {
        self.0.get(name).is_some_and(|s| s.0)
    }
}

/// CreateButton's GameObject (55 by 55, the preserveAspect icon) and, when it has children, CreateChildContainerFor's
/// container: anchored at the button's left middle, pivot (1, 0.5), black, a HorizontalLayoutGroup (spacing 15, padding
/// 10) fitted to its children, inactive at first. The wrapper is the hover area, as the container is the button's child.
fn tool_button(p: &mut Kids, b: &ToolButton, art: &UiArt, in_flyout: Option<&str>) {
    let ToolDef { name, description, ty, .. } = b.tool;
    p.spawn((Node { width: px(55), height: px(55), flex_shrink: 0.0, align_items: AlignItems::Center, ..default() }, Hovered::default())).with_children(|w| {
        let mut e = w.spawn((Node { width: px(55), height: px(55), flex_shrink: 0.0, ..default() }, BackgroundColor(CLEAR), Hovered::default(), Look { sel: Sel::Tool(name.clone()), on: true }, Tip(tooltip(name, description), true), Press(Act::Tool(name.clone(), ty.clone()))));
        if let Some(parent) = in_flyout {
            e.insert(InFlyout(parent.to_string()));
        }
        e.with_children(|i| {
            if let Some(a) = art.tools.get(name) {
                picture(i, a, 55.0, 55.0, WHITE);
            }
        });
        if !b.children.is_empty() {
            w.spawn((Node { position_type: PositionType::Absolute, right: percent(100), display: Display::None, padding: UiRect::all(px(10)), ..row(15.0) }, BackgroundColor(BLACK), Flyout(name.clone()))).with_children(|f| {
                for k in &b.children {
                    tool_button(f, k, art, Some(name));
                }
            });
        }
    });
}

/// LayerEditorController's Container: the layer and order with an arrow pair each, above the targets.
fn layer_editor(root: &mut Kids, x: f32, y: f32, layer: &str, order: &str) {
    root.spawn((Node { position_type: PositionType::Absolute, left: px(x - 90.0), bottom: px(y + 10.0), width: px(180), padding: UiRect::all(px(8)), ..column(4.0) }, BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.86)))).with_children(|m| {
        for (name, text, is_layer) in [("Layer", layer, true), ("Order", order, false)] {
            m.spawn(row(8.0)).with_children(|r| {
                label(r, format!("{name}: {text}"), 22.0, WHITE);
                text_button(r, "v", 22.0, Some(Act::LayerOffset(is_layer, false)), Sel::None, "", Node { width: px(30), justify_content: JustifyContent::Center, ..default() });
                text_button(r, "^", 22.0, Some(Act::LayerOffset(is_layer, true)), Sel::None, "", Node { width: px(30), justify_content: JustifyContent::Center, ..default() });
            });
        }
    });
}

/// Canvas/Catalog/Environment options: black over the whole catalog (a later sibling, so above it), the Desc text on top,
/// the options scroll view, the Tooltip box (its TextMeshPro Text is EnvironmentSettingsController.TooltipTarget) and the
/// settings row with the Back button (Toybox button: OnClick SetActive(false) on the window).
const ENV: &str = "Canvas/Catalog/Environment options";

/// EnvironmentSettingsController.RegenerateControls: one control per EnvironmentalSettings field in declaration order
/// (ENVIRONMENT), CreateToggle for a bool and CreateNumberInput for a float, into Container (options/Viewport/Content:
/// VerticalLayoutGroup, left padding 15, children's width controlled, their own 64 height).
/// EnvironmentSettingsController.PrepareControl: Label.text = the field name with "_" as " ", Tooltip.Text = its
/// TooltipAttribute, Tooltip.TooltipText = TooltipTarget, so hovering a row shows AppendPunctuation(tooltip) in the Tooltip box.
/// EnvironmentSettingsController.RegenerateControls (its Start at the map's load, and Global.SetPausedMenu(false)), as
/// uifn checks: the rows, CreateToggle's isOn and CreateNumberInput's text of the last of each, PrepareControl's label and
/// tooltip of the last row. The rows themselves are `environment`, drawn from the same values.
/// Every call but the window's first finds the old rows in Container: Object.Destroy removes them only at the frame's end,
/// so Container then holds them and the new ones (Start's first call at the load has none).
fn environment_regenerated(values: &crate::game::map::EnvironmentalSettings, unit: i32) {
    let names = ENVIRONMENT.iter().map(|e| e.0).collect::<Vec<_>>().join("|");
    super::uifn("EnvironmentSettingsController.RegenerateControls", || format!("{} {names}|{names}", 2 * ENVIRONMENT.len()));
    for (i, &(name, is_bool, tip, _, _, temperature)) in ENVIRONMENT.iter().enumerate() {
        if is_bool {
            super::uifn("EnvironmentSettingsController.CreateToggle", || format!("{name} {}", super::cs(values.field(i) != 0.0)));
        } else {
            super::uifn("EnvironmentSettingsController.CreateNumberInput", || format!("{name} {}", crate::game::map::environment_number_text(values, i, temperature, unit)));
        }
        super::uifn("EnvironmentSettingsController.PrepareControl", || format!("{name} {}", super::ui_text(tip)));
    }
}

fn environment(root: &mut Kids, ui: &Ui, settings: &Settings) {
    let Some(values) = ui.env_values else { return };
    let unit = settings.get("TemperatureUnit") as i32;
    let Some(values) = ui.env_values else { return };
    let desc = scene::rect("Main", &format!("{ENV}/Desc")).size_delta[1];
    let options = scene::rect("Main", &format!("{ENV}/options"));
    let tooltip = scene::rect("Main", &format!("{ENV}/Tooltip"));
    let bar = scene::rect("Main", &format!("{ENV}/settings")).size_delta[1];
    // options: stretched, inset at the top by -sizeDelta/2 - anchoredPosition and at the bottom by -sizeDelta/2 + anchoredPosition.
    let (top, bottom) = (-options.size_delta[1] / 2.0 - options.anchored_position[1], -options.size_delta[1] / 2.0 + options.anchored_position[1]);
    let width = catalog_width(ui, settings);
    root.spawn((Node { position_type: PositionType::Absolute, left: px(0), top: px(0), bottom: px(0), width: px(width), ..default() }, BackgroundColor(scene::image_color("Main", ENV)))).with_children(|m| {
        // Desc: TextMeshPro 23, top left, margins 15 (TMP turns the literal \n into a line break).
        m.spawn(Node { position_type: PositionType::Absolute, left: px(0), right: px(0), top: px(0), height: px(desc), padding: UiRect::all(px(15)), ..column(0.0) }).with_children(|d| {
            rich(d, &scene::text("Main", &format!("{ENV}/Desc")).replace("\\n", "\n"), 23.0, Justify::Left);
        });
        // options: a vertical ScrollRect (clamped) with no Scrollbar; Content's VerticalLayoutGroup pads 15 on the left.
        m.spawn(Node { position_type: PositionType::Absolute, left: px(0), right: px(0), top: px(top), bottom: px(bottom), ..column(0.0) }).with_children(|o| scroll_area(o, ui, 11, Node { flex_grow: 1.0, padding: UiRect::left(px(15)), overflow: Overflow::scroll_y(), ..column(0.0) }, |c| {
            for (i, &(name, is_bool, tip, _, _, temperature)) in ENVIRONMENT.iter().enumerate() {
                // The prefab's root carries the HasTooltipBehaviour: the whole row shows the tip.
                c.spawn((Node { height: px(64), flex_shrink: 0.0, ..row(0.0) }, Hovered::default(), EnvTip(tip.into()))).with_children(|r| {
                    // Label: TextMeshPro 20, left, middle, margins 5, in the Label container at localScale k. Both prefabs'
                    // HorizontalLayoutGroups use childScale: the container counts at k, its flexible width too, so it
                    // takes k x flexible of the share (its unscaled width drawn at k), the full height drawn at k, centred.
                    let k = scene::rect("Number setting", "Label container").scale[0];
                    let text = |r: &mut Kids, grow: f32| {
                        r.spawn((Node { flex_grow: grow * k, flex_basis: px(0), min_width: px(0), height: px(64.0 * k), align_self: AlignSelf::Center, ..default() }, Pickable::IGNORE)).with_children(|l| {
                            l.spawn((Node { flex_grow: 1.0, min_width: px(0), padding: UiRect::all(px(5.0 * k)), ..default() }, Pickable::IGNORE)).with_children(|l| label(l, name, 20.0 * k, WHITE));
                        });
                    };
                    if is_bool {
                        // Checkbox setting.prefab: the label container flexible 1000, the box container 64 wide (flexible 1 by the
                        // HorizontalLayoutGroup's force expand). Box: a white 32 square whose Toggle tints it (normal 0.24, highlighted
                        // 0.32, pressed 0.19, no fade); its graphic is the green 32 square Toggled (centred, localScale 0.5),
                        // shown while isOn.
                        text(r, 1000.0);
                        let toggled = scene::rect("Checkbox setting", "Box container/Box/Toggled");
                        let mark = toggled.size_delta[0] * toggled.scale[0];
                        r.spawn((Node { flex_grow: 1.0, flex_basis: px(64), align_self: AlignSelf::Stretch, justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|b| {
                            let tint = Tint::new([1.0; 4], [0.24, 0.24, 0.24, 1.0], [0.32, 0.32, 0.32, 1.0], [0.19, 0.19, 0.19, 1.0], 0.0);
                            b.spawn((Node { width: px(32), height: px(32), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, tint.bundle(), Hovered::default(), Press(Act::EnvToggle(i)))).with_children(|t| {
                                if values.field(i) != 0.0 {
                                    t.spawn((Node { width: px(mark), height: px(mark), ..default() }, BackgroundColor(CHECKED), Pickable::IGNORE));
                                }
                            });
                        });
                    } else {
                        // Number setting.prefab: label and box containers share the row (both flexible 1 by force expand). InputField
                        // (TMP): inset 15, its Text Area Image white tinted 0.239 in every state; Text 23, centred, middle.
                        text(r, 1.0);
                        let value = match &ui.env_edit {
                            Some(e) if e.row == i => format!("{}|", e.text),
                            _ => crate::game::map::environment_number_text(&values, i, temperature, unit),
                        };
                        r.spawn((Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), align_self: AlignSelf::Stretch, ..default() }, Pickable::IGNORE)).with_children(|b| {
                            let g = 0.23921569;
                            b.spawn((Node { flex_grow: 1.0, min_width: px(0), margin: UiRect::all(px(15)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, overflow: Overflow::clip(), ..default() }, BackgroundColor(Color::srgb(g, g, g)), Hovered::default(), Press(Act::EnvEdit(i)))).with_children(|t| {
                                t.spawn((Text::new(value), font(23.0), TextColor(WHITE), TextLayout { justify: Justify::Center, ..default() }, Pickable::IGNORE));
                            });
                        });
                    }
                });
            }
        }));
        // Tooltip: bottom anchored, pivot top, its top edge anchoredPosition.y above the bottom; TooltipTarget: TextMeshPro 18,
        // top left, margins 2. HasTooltipBehaviour.OnPointerExit hides the text, the black box stays.
        m.spawn((Node { position_type: PositionType::Absolute, left: px(0), right: px(-tooltip.size_delta[0]), bottom: px(tooltip.anchored_position[1] - tooltip.size_delta[1]), height: px(tooltip.size_delta[1]), padding: UiRect::all(px(2)), ..default() }, BackgroundColor(scene::image_color("Main", &format!("{ENV}/Tooltip"))), Pickable::IGNORE)).with_children(|t| {
            bound(t, Bound::EnvTip, 18.0, WHITE);
        });
        // settings: a VerticalLayoutGroup (its padding) the Back button fills. Its text "Back\n" 23, centred, middle, margins 5.
        let pad = scene::padding("Main", &format!("{ENV}/settings"), "VerticalLayoutGroup");
        m.spawn(Node { position_type: PositionType::Absolute, left: px(0), right: px(0), bottom: px(0), height: px(bar), padding: UiRect { left: px(pad[0]), right: px(pad[1]), top: px(pad[2]), bottom: px(pad[3]) }, ..default() }).with_children(|s| {
            s.spawn((Node { flex_grow: 1.0, padding: UiRect::all(px(5)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }, Tint::of("Main", &format!("{ENV}/settings/Toybox button")).bundle(), Hovered::default(), Press(Act::Environment(false)))).with_children(|b| {
                b.spawn((Text::new(scene::text("Main", &format!("{ENV}/settings/Toybox button/TextMeshPro Text"))), font(23.0), TextColor(WHITE), TextLayout { justify: Justify::Center, ..default() }, Pickable::IGNORE));
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The layout numbers and colours read from the install equal the literals they replaced (skipped without an install).
    #[test]
    fn scene_numbers_equal_the_old_literals() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = crate::assets::db::AssetDb::open(&game).unwrap();
        scene::load(&mut db).unwrap();
        let near = |a: f32, b: f32| assert!((a - b).abs() < 0.01, "{a} != {b}");
        near(scene::rect("Menu", INFO).size_delta[0], 1325.2);
        near(scene::rect("Menu", INFO_TEXT).size_delta[1], 133.3773);
        near(scene::rect("Menu", INFO_LINKS).size_delta[1], 139.558);
        near(scene::rect("Menu", INFO_VERSION).size_delta[1], 123.5245);
        near(scene::rect("Menu", "Canvas/Information/Info/discord").size_delta[1], 58.3162);
        near(scene::spacing("Menu", INFO, "VerticalLayoutGroup"), 16.13);
        near(scene::rect("Menu", INFO).size_delta[0] + scene::rect("Menu", "Canvas/Information/Info/button/Button").size_delta[0], 458.62);
        near(scene::vec2("Main", "Canvas/Catalog/ItemPanel", "GridLayoutGroup", "m_CellSize")[0], 147.87);
        near(scene::rect("Main", "Canvas/Catalog/CategoryContainer/MainCategoryPanel").size_delta[1], 66.4);
        near(scene::rect("Main", "Canvas/Catalog/CategoryContainer/Filter").size_delta[1], 59.89);
        near(scene::rect("Main", "Canvas/Catalog/settings").size_delta[1], 151.78);
        near(scene::rect("Main", "Canvas/Status bar/Follow Status").size_delta[0], 272.61);
        near(scene::rect("Main", "Canvas/Status bar").size_delta[1], 66.61);
        near(scene::rect("Main", "Canvas/Toolbar/tab-tools").anchored_position[1], 303.36);
        near(scene::rect("Main", "Canvas/Limb status view").size_delta[0], 551.976);
        near(scene::placed("Menu", "Canvas/Map Selection/Map menu", [0.0, 0.0])[0], 281.26);
        near(handle_color("Hover").to_srgba().alpha, 0.8);
        near(handle_range().0[1], 1863.555);
        near(dialog_button().normal[0], 0.26415092);
        near(Tint::of("Menu", "Canvas/Information/Info/button/Button").image[3], 0.7607843);
        // Everything the dialogs read exists.
        let _ = (dialog_outline(), Tint::of_graphic("Main", "Canvas/Dialog container/Colorpicker/Colourpicker/InputField (TMP)", "TMP_InputField", "Canvas/Dialog container/Colorpicker/Colourpicker/InputField (TMP)/Text Area"));
        near(scene::placed("Main", "Canvas/Dialog container/Colorpicker/Colourpicker/Saturation Value box", [595.1694, 602.9094])[0], 18.87);
    }

    #[test]
    fn rich_lines_carry_styles_over_newlines_and_keep_unknown_tags() {
        let l = rich_lines("<color=#aaa>a <x@y.z>\nb</color>\n<size=70%>c</size>", 20.0);
        assert_eq!(l.len(), 3);
        assert_eq!(l[0][0].0, "a <x@y.z>");
        assert_eq!(l[0][0].1.color, tmp_color("#aaaaaa").unwrap());
        assert_eq!(l[1][0].1.color, l[0][0].1.color);
        assert_eq!((l[2][0].0.as_str(), l[2][0].1.size), ("c", 14.0));
        if let Ok(game) = crate::game_assets::find_game(None) {
            scene::load(&mut crate::assets::db::AssetDb::open(&game).unwrap()).unwrap();
            assert_eq!(rich_lines(&scene::text("Menu", LICENSES_PATH).replace("\r\n", "\n"), 18.0).iter().filter(|l| l.first().is_some_and(|r| r.1.bold)).count(), 8);
        }
    }

    #[test]
    fn rich_runs_split_bold_and_colour() {
        let r = rich_runs("Now playing <color=#ff54f9>Song</color> <b>X</b> <i>");
        let pink = Color::srgb(1.0, 84.0 / 255.0, 249.0 / 255.0);
        assert_eq!(r, vec![("Now playing ".into(), false, WHITE), ("Song".into(), false, pink), (" ".into(), false, WHITE), ("X".into(), true, WHITE), (" <i>".into(), false, WHITE)]);
    }
}
