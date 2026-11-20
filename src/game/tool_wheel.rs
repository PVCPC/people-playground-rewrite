//! ToolWheelBehaviour: the Tools / Powers tab and the tool visibility, switched by InputSystem keys, and the eye's
//! UiColorWave. The wheel's scale when toggled and the tab objects are the toolbar view's (ui/view.rs).
use super::{d, mathf, Game};

/// The eye's UiColorWave (EyeAnimation on Canvas/Toolbar/toggle-visibility/eye_open) and EyeImage's colour it sets.
#[derive(Clone, Debug, Default)]
pub struct EyeAnimation {
    /// Behaviour.enabled (the scene's starts off).
    pub enabled: bool,
    oV: f32,
    sV: f32,
    /// Gradient, Duration, Offset, UseSinWave, OffsetVaration, SpeedVaration, Randomisation (None without the scene's data).
    wave: Option<(super::visual::Gradient, f32, f32, bool, f32, f32, f32)>,
    /// EyeImage's Image.color; its sprite is EyeClosed while the tools are invisible.
    pub color: [f32; 4],
}

impl EyeAnimation {
    /// The scene's UiColorWave fields and EyeImage's m_Color.
    pub fn read(wave: &crate::assets::mono::Value, image: &crate::assets::mono::Value) -> Result<Self, String> {
        use crate::assets::mono::Value;
        let f = |n: &str| wave.get(n).and_then(Value::f32).ok_or_else(|| format!("UiColorWave.{n}"));
        let gradient = super::visual::Gradient::read(wave.get("Gradient").ok_or("UiColorWave.Gradient")?)?;
        let sin = wave.get("UseSinWave").and_then(Value::bool).ok_or("UiColorWave.UseSinWave")?;
        let c = image.get("m_Color").ok_or("Image.m_Color")?;
        let ch = |k: &str| c.get(k).and_then(Value::f32).ok_or("Image.m_Color");
        Ok(EyeAnimation { enabled: false, oV: 0.0, sV: 0.0, wave: Some((gradient, f("Duration")?, f("Offset")?, sin, f("OffsetVaration")?, f("SpeedVaration")?, f("Randomisation")?)), color: [ch("r")?, ch("g")?, ch("b")?, ch("a")?] })
    }
}

impl Game {
    /// ToolWheelBehaviour.Update
    pub fn tool_wheel_update(&mut self) {
        if self.tool.is_none() {
            return;
        }
        // !DialogBox.IsAnyDialogboxOpen && !Global.ActiveUiBlock && Global.main.UiEnabled && !GetPausedMenu().
        if self.input.ui_block || self.input.ui_hidden || self.paused_menu {
            return;
        }
        // "toybox": Toggle() only scales the wheel (presentation).
        if self.input.up("toolPowerToggle") {
            // (int)(CurrentTab + 1) % Enum.GetValues(typeof(ToolTab)).Length
            let tab = (self.tool.as_ref().unwrap().tab + 1) % 2;
            self.tool_wheel_set_tab(tab);
            self.notify(format!("Changed to {} tab", if tab == 0 { "Tools" } else { "Powers" }));
        }
        if self.input.down("toolInvisibility") {
            // UISoundBehaviour.Main.Down()
            self.ui_sound(super::audio::UiSoundKind::Down);
            self.tool_wheel_set_tool_visibility(true);
        } else if self.input.up("toolInvisibility") {
            self.tool_wheel_set_tool_visibility(false);
        }
    }

    /// ToolWheelBehaviour.SetTab(int) and SetTab(ToolTab): CurrentTab = tab; the Tabs objects' SetActive is the toolbar's list (ui/mod.rs).
    pub fn tool_wheel_set_tab(&mut self, tab: i32) {
        self.tool.as_mut().unwrap().tab = tab;
    }

    /// ToolWheelBehaviour.SetToolVisibility: ToolVisibility, EyeAnimation.enabled, EyeImage.sprite (EyeClosed while
    /// `invisible`, drawn by the toolbar view), then OnToolVisibilityChanged: the ToolInvisibilityTrackers (tool_invisibility.rs).
    pub fn tool_wheel_set_tool_visibility(&mut self, invisible: bool) {
        crate::ui::uifn("ToolWheelBehaviour.set_ToolVisibility", || visibility(invisible).into());
        self.tool.as_mut().unwrap().invisible = invisible;
        if self.eye.enabled != invisible {
            self.eye.enabled = invisible;
            if invisible {
                self.eye_animation_on_enable();
            } else {
                self.eye_animation_on_disable();
            }
        }
        self.trackers_on_tool_visibility_changed();
        // The per-function check: ToolVisibility and EyeAnimation.enabled after.
        crate::ui::uifn("ToolWheelBehaviour.SetToolVisibility", || format!("{} {}", visibility(invisible), if self.eye.enabled { "True" } else { "False" }));
    }

    /// UiColorWave.OnEnable (the eye's): oV = Random.value, sV = Random.value.
    pub(crate) fn eye_animation_on_enable(&mut self) {
        self.eye.oV = self.random.value();
        self.eye.sV = self.random.value();
    }

    /// UiColorWave.OnDisable (the eye's): Renderer.color = Gradient.Evaluate(0).
    pub(crate) fn eye_animation_on_disable(&mut self) {
        if let Some((g, ..)) = &self.eye.wave {
            self.eye.color = g.evaluate(0.0);
        }
    }

    /// UiColorWave.Update (the eye's, while enabled): Renderer.color = Gradient.Evaluate of the wave at Time.time.
    pub fn eye_animation_update(&mut self) {
        if !self.eye.enabled {
            return;
        }
        let Some((g, duration, offset, sin, ov_var, sv_var, randomisation)) = &self.eye.wave else { return };
        if !(*duration > 0.0) {
            return;
        }
        let (ov, sv) = (self.eye.oV, self.eye.sV);
        // The same arithmetic as the sprites' wave (visual.rs visual_will_render).
        // 1.28.45 reads Time.unscaledTime.
        let time = if crate::assets::il::is_1_28_45() { self.time.unscaledTimeAsDouble as f32 } else { self.time.time };
        let num = (d((d(time) * (d(1.0) + d(sv) * *sv_var) / *duration).f()) + *offset + d(ov) * *ov_var).f();
        let mut num2 = if *sin { mathf::sin(num) * 0.5 + 0.5 } else { super::physical::utils_mod(num, 1.0) };
        if *randomisation != 0.0 {
            num2 += mathf::perlin2_mapped(time / *duration, ov * 423.0)[0];
        }
        self.eye.color = g.evaluate(num2);
    }

    /// ToolInvisibilityTracker.get_ShouldBeVisible, for a live owner: an Invisible one is visible (and deletable) only while the tools are invisible too.
    /// The tracker's other members are tool_invisibility.rs.
    /// No tool records (a recording's case without them): ToolWheelBehaviour.Instance's ToolVisibility is its default Visible
    /// (only SetToolVisibility, the eye button, changes it).
    pub fn tool_should_be_visible(&self, owner_invisible: bool) -> bool {
        !owner_invisible || self.tool.as_ref().is_some_and(|t| t.invisible)
    }

    /// ToolWheelBehaviour.ToggleToolVisibility: SetToolVisibility(ToolVisibility.Opposite()).
    pub fn tool_wheel_toggle_tool_visibility(&mut self) {
        let invisible = self.tool.as_ref().unwrap().invisible;
        // ToolVisibility is the bool `invisible` (Visibility.Invisible = 1).
        let invisible = visibility_opposite(invisible as i32) == 1;
        self.tool_wheel_set_tool_visibility(invisible);
        crate::ui::uifn("ToolWheelBehaviour.ToggleToolVisibility", || visibility(invisible).into());
    }
}

/// VisibilityExtensions.Opposite: Visible (0) -> Invisible (1), anything else -> Visible.
pub(crate) fn visibility_opposite(v: i32) -> i32 {
    if v == 0 { 1 } else { 0 }
}

/// SelectionBoxBehaviour.Snap: Mathf.Round(100 * |i|) / 100 (Mathf.Round is Math.Round: ties to even), for the size labels.
pub(crate) fn selection_box_snap(i: f32) -> f32 {
    (100.0 * i.abs()).round_ties_even() / 100.0
}

/// The Visibility enum's name for Tool::invisible.
pub(crate) fn visibility(invisible: bool) -> &'static str {
    if invisible { "Invisible" } else { "Visible" }
}

// ToolWheelBehaviour.Awake: Instance = this.
// ToolWheelBehaviour.Instance: that field (get_Instance and set_Instance).
// ToolWheelBehaviour.Start: localScale = 1.2 (the toolbar view's UiTransform, ui/view.rs toolbar) and SetTab(ToolTab.Tools), the Tool's initial tab 0.
// ToolWheelBehaviour.Toggle: isOpen and the wheel's localScale, the UI's `hide_panels` (ui/mod.rs hotkeys) and the toolbar view.
// ToolWheelBehaviour.ToolVisibility: Tool::invisible (get_ToolVisibility and set_ToolVisibility, set by SetToolVisibility).
// ToolWheelBehaviour.constructor: field initialisers (isOpen true: `hide_panels` starts false).

// SelectionBoxBehaviour.OnEnable: Main = this, the UI box the tool draws.
// SelectionBoxBehaviour.Awake: GetComponent<Image>() and <RectTransform>().
// SelectionBoxBehaviour.Start: Hide().
// SelectionBoxBehaviour.Show: Image and the size labels enabled.
// SelectionBoxBehaviour.Hide: Image and the size labels disabled.
// SelectionBoxBehaviour.SetSize: rectTransform.sizeDelta.
// SelectionBoxBehaviour.SetSizeDisplay: the labels' text, with Snap.
// (All SelectionBoxBehaviour methods are presentation: a UI image and TextMeshPro labels that the selection in tool.rs never reads.)
// ToolButtonBehaviour.Awake: toolController = FindObjectOfType, the one ToolController (`Game::tool`).
// ToolButtonBehaviour.SetTool: toolController.SetToolByTypeName(BehaviourName), set_tool_by_type_name; only a UI click calls it.
// ToolLibrary.Awake: Instance = this.
// ToolLibrary.Instance: that field (get_Instance and set_Instance); the lists are read from the scene by scene.rs.
// ToolLibrary.static setup: OnCollectionChange and ModdedTypes, the listener list and the mod type set (mods are out of scope).
// ToolLibrary.BroadcastCollectionChange: OnCollectionChange.Invoke(), whose only listener is ToolBarGenerator.GenerateButtons; only mods add tools, so the lists never change after load.
// ToolBarGenerator.Start: GenerateButtons and the listener: the toolbar view (ui/view.rs toolbar_wheel) built from the scene's lists.
// ToolBarGenerator.GenerateButtons: the toolbar view's rebuild of the Tools and Powers containers (ui/view.rs toolbar_wheel).
// ToolBarGenerator.GenerateButtonsFor: the parentless entries of a list, one button each (ui/view.rs toolbar_wheel).
// ToolBarGenerator.CreateButton: ui/view.rs tool_button (preserveAspect icon, 55 by 55, the "<b>Name</b>\nDescription" tooltip, onClick SetToolByTypeName) and the child container's hover and click (ui/mod.rs live and clicks).
// ToolBarGenerator.CreateChildContainerFor: ui/view.rs Flyout (black, HorizontalLayoutGroup spacing 15 padding 10, inactive at first).
