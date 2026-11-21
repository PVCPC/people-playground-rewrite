//! Global.Update, whole (Global.cs): the live game's one path for its keys (ui/mod.rs `world_input` hands them over in
//! `input.controls`, F1 in `input.keys`; `game_frame` reads back what changed), recordings and the per-function checks.
use super::Game;

impl Game {
    /// Global.Start, the Global component `id`: the UI blockers reset, the pause menu closed, the time scale target 1 and the
    /// sources' pitch, again after 0.05 s (DelayCoroutine), then the Steam stats (stats::global_start). eventSystem, CanvasScaler
    /// and FireLoopSoundControllerBehaviour are lookups of fixed scene objects; the overlay subscription is SteamFriends'.
    pub fn global_start(&mut self, id: i32) {
        self.ActiveUiBlockers = 0;
        self.set_paused_menu(false, "Main");
        self.timeScaleTarget.0 = 1.0;
        self.audio.update_pitch(self.timeScaleTarget.0);
        self.start_wait(id, 0.05, super::destroyable::Step::GlobalUpdatePitch);
        super::stats::global_start();
    }

    /// Global.Update
    pub fn global_update(&mut self) {
        // SteamworksInitialiser.IsInitialised (the local store, unless `steam_off`): an hour without a kill while the player
        // was active (isActivePlayer: inactivity under 300 s) counts PACIFIST once (Time.timeSinceLevelLoad).
        if super::stats::STEAMWORKS_INITIALISED && !self.steam_off && !self.alreadyUnlockedPacifist {
            if self.inactivity >= 300.0 {
                super::stats::set_last_kill_time(self.time.time_since_level_load());
            } else if self.time.time_since_level_load() - super::stats::last_kill_time() > 3600.0 {
                super::stats::increment_integer(super::stats::PACIFIST, 1);
                self.alreadyUnlockedPacifist = true;
            }
        }
        if !self.trigger_being_edited && !self.global_active_ui_block() {
            // "pause" toggles the pause menu: no dialog box open, and for an Escape binding neither the resize handles nor
            // the layer editor up; an open menu closes only from its Main page.
            let escape = match &self.control_binds {
                Some(b) => crate::ui::tables::ACTIONS.iter().position(|a| a.id == crate::il!(str "Global::Update" 0)).is_some_and(|i| b[i].0 == "Escape"),
                None => crate::ui::tables::ACTIONS.iter().any(|a| a.id == crate::il!(str "Global::Update" 0) && a.key == "Escape"),
            };
            if self.input.down(crate::il!(str "Global::Update" 0))
                && !self.dialog_open
                && (!escape || (!self.resize.active && !self.layer_editor_is_open()))
                && (!self.paused_menu || self.menu_page == "Main")
            {
                self.set_paused_menu(!self.paused_menu, "Main");
            }
            // Input.GetKeyDown(KeyCode.F1)
            if self.input.keys.get("F1").is_some_and(|c| c.down) {
                self.set_paused_menu(true, "Controls");
            }
            self.process_snapping_shortcuts();
        }
        // else "pause" clears EventSystem.current's selection (UI focus).
        // Global.UpdateMousePosition: MousePosition = the camera's ScreenToWorldPoint of the mouse (`input.mouse`).
        let dt = self.time.unscaledDeltaTime;
        self.oldMousePos = self.MousePosition;
        self.MousePosition = self.input.mouse;
        let target = if self.paused || self.paused_menu { 0.0 } else { self.timeScaleTarget.0 };
        let old = self.time.timeScale;
        self.time.timeScale = super::fire::smooth_approach(old, target, 15.0, dt);
        if (target - old).abs() > 0.0001 {
            self.audio.update_pitch(old);
        }
        if self.paused_menu || self.dialog_open || self.global_active_ui_block() {
            return;
        }
        if self.input.down("toggleLimbStatus") {
            self.toggle_limb_status();
        }
        if self.input.down(crate::il!(str "Global::Update" 7)) {
            self.toggle_slowmotion();
        }
        if self.input.down(crate::il!(str "Global::Update" 8)) {
            self.toggle_paused();
        }
        // ContextMenuBehaviour.Show(Input.mousePosition): its world position is `context_at`; Hide closes an open menu.
        for (id, button) in [("freezeSelection", "Freeze"), ("igniteSelection", "Ignite"), ("resizeSelection", "Resize")] {
            if self.input.down(id) && self.context_hotkey(button, self.context_at) {
                self.context_hidden = true;
            }
        }
        // CatalogBehaviour.SetItem (SetCategory is the UI's catalogue page) and UISoundBehaviour.Blip.
        if self.input.down(crate::il!(str "Global::Update" 12)) {
            if let Some(name) = self.eyedrop_asset() {
                self.catalog_selected = Some(name);
                self.ui_sound(super::audio::UiSoundKind::Blip);
            }
        }
        if self.input.down("toggleHoveringHighlights") {
            self.prefs.ShowOutlines = !self.prefs.ShowOutlines;
            self.notify(format!("Hovering highlights have been {}", if self.prefs.ShowOutlines { crate::il!(str "Global::Update" 16) } else { crate::il!(str "Global::Update" 15) }));
            // UserPreferenceManager.Save writes the settings file: the UI's.
        }
    }

    /// Global.ProcessSnappingShortcuts: GridSnap steps by 0.1 within 0.1..2 and AngleSnap by 5 within 5..90, each saved and announced.
    pub(super) fn process_snapping_shortcuts(&mut self) {
        use super::mathf::clamp;
        for (id, delta) in [("incrementGrid", 0.1f32), ("decrementGrid", -0.1)] {
            if self.input.down(id) {
                self.prefs.GridSnap = clamp(self.prefs.GridSnap + delta, 0.1, 2.0);
                // Math.Round((double)GridSnap, 2): rounded to even at two digits, printed shortest.
                self.notify(format!("Grid size set to {} m", ((self.prefs.GridSnap as f64) * 100.0).round_ties_even() / 100.0));
            }
        }
        for (id, delta) in [("incrementAngleSnap", 5), ("decrementAngleSnap", -5)] {
            if self.input.down(id) {
                self.prefs.AngleSnap = (self.prefs.AngleSnap + delta).clamp(5, 90);
                self.notify(format!("Angle snap set to {} degrees", self.prefs.AngleSnap));
            }
        }
    }

    /// Global.SetPausedMenu: the menu's open state and MenuController.SetPage(page). The video and mixer Sync, the canvas
    /// and the environment controls' regeneration are the UI's.
    pub fn set_paused_menu(&mut self, value: bool, page: &str) {
        self.paused_menu = value;
        self.menu_page = page.to_string();
    }

    /// Global.ToggleSlowmotion
    pub fn toggle_slowmotion(&mut self) {
        self.SlowMotion = !self.SlowMotion;
        self.notify(if self.SlowMotion { crate::il!(str "Global::ToggleSlowmotion" 1) } else { crate::il!(str "Global::ToggleSlowmotion" 0) }.into());
        self.timeScaleTarget.0 = if self.SlowMotion { self.prefs.SlowMotionSpeed / 100.0 } else { 1.0 };
    }

    /// Global.TogglePaused: its DelayCoroutine is never started (Utils.DelayCoroutine only returns the IEnumerator).
    pub fn toggle_paused(&mut self) {
        self.set_paused(!self.paused);
        self.notify(if self.paused { "Paused" } else { "Unpaused" }.into());
    }
}
