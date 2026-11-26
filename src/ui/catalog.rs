//! CatalogBehaviour's contraption half (the selected contraption, its outline, the mods of spawnables) and ItemButtonBehaviour.
#![allow(non_snake_case)]

use super::{
    data::Item,
    state::{DialogRun, DialogView, Ui},
};
use crate::game::{
    contraption::{self, ContraptionMetaData, RotatedRectangle},
    save::ObjectState,
};
use std::collections::HashMap;

/// IModInfo, as far as the catalog uses it (ContraptionSerialiser.SaveContraption lists these as the required mods).
#[derive(Clone, Debug, PartialEq)]
pub struct ModInfo {
    pub Name: String,
    pub Identifier: String,
    pub WorkshopId: Option<String>,
}

/// What CatalogBehaviour keeps about the selected contraption and the mods' spawnables.
#[derive(Default)]
pub struct LocalCatalog {
    /// SelectedContraption: its ObjectStates.
    pub selected: Option<Vec<ObjectState>>,
    /// SelectedContraptionMetaData
    pub meta: Option<ContraptionMetaData>,
    /// SpawnableOutline: the outline's rectangles and its localScale.x (1, or -1 flipped), while shown.
    pub outline: Option<(Vec<RotatedRectangle>, f32)>,
    /// secondsSpawnKeyHeld
    pub seconds_spawn_key_held: f32,
    /// The selection changed since the game last took it (SelectionChanged).
    pub changed: bool,
    /// spawnableByMod: the mod each modded spawnable came from (ApplyModification fills it; mods are not loaded).
    pub spawnable_by_mod: HashMap<String, ModInfo>,
}

/// CatalogBehaviour.secondsSpawnKeyHeldThreshold
const SPAWN_KEY_HELD_THRESHOLD: f32 = 0.1;

impl Ui {
    /// CatalogBehaviour.get_Contraptions: the read-only view of `contraptions`, the item buttons that carry a saved contraption.
    pub fn contraptions(&self) -> impl Iterator<Item = &Item> {
        let n = self.catalog.items.iter().filter(|i| i.local.is_some()).count();
        super::uifn("CatalogBehaviour.get_Contraptions", || n.to_string());
        self.catalog.items.iter().filter(|i| i.local.is_some())
    }

    /// CatalogBehaviour.SetContraption: the contraption selected, no spawnable (SelectionChanged is `catalog_selected` following `item`).
    pub fn set_contraption(&mut self, contraption: Vec<ObjectState>) {
        self.local.selected = Some(contraption);
        self.local.changed = true;
        self.item = None;
    }

    /// ItemButtonBehaviour.OnPointerDown for a saved contraption (button `i` of the catalog; nothing while a dialog box is open): when it requires mods
    /// that are not enabled (none is, ModLoaderRegistry.Mods being empty) a dialog lists them with Cancel and Select anyway, else it is loaded.
    pub fn select_contraption(&mut self, i: usize, now: f64) {
        if self.dialog.is_some() {
            return;
        }
        let Some(meta) = self.catalog.items.get(i).and_then(|it| it.local.as_ref()) else { return };
        let missing: Vec<String> = meta.RequiredMods.iter().flatten().map(|m| m.Name.clone().unwrap_or_default()).collect();
        if meta.RequiresMods() && !missing.is_empty() {
            // UISoundBehaviour.Main.Warning() is audio.
            let mut dialog = DialogView::notification(format!("Unfulfilled mod requirements:
{}", missing.join("
")));
            dialog.buttons = vec![("Cancel".into(), true, None), (crate::il!(str "ItemButtonBehaviour::OnPointerDown" 3).into(), true, Some(DialogRun::SelectContraption(i)))];
            dialog.set_height((missing.len() * 41 + 200) as f32);
            self.open_dialog(dialog);
            return;
        }
        self.load_selected_contraption(i, now);
    }

    /// ItemButtonBehaviour.OnPointerDown's `load`: ContraptionSerialiser.LoadContraption of the button's metadata; a failure shows "This contraption is
    /// corrupted" (or the incompatible-version text) and selects nothing, else SelectedContraptionMetaData and SetContraption.
    pub fn load_selected_contraption(&mut self, i: usize, now: f64) {
        let Some(mut meta) = self.catalog.items.get(i).and_then(|it| it.local.clone()) else { return };
        let mut messages = contraption::LoadMessages::default();
        match contraption::load_contraption(&mut meta, &mut messages) {
            Ok(states) => {
                for n in messages.notifications {
                    self.notify(n, now);
                }
                if let Some(d) = messages.dialogs.into_iter().next() {
                    self.open_dialog(DialogView::notification(d));
                }
                self.local.meta = Some(meta);
                self.set_contraption(states);
            }
            Err(e) => {
                eprintln!("{e}");
                // UISoundBehaviour.Main.Error() is audio.
                self.open_dialog(DialogView::notification(contraption::load_failure_notification(&meta)));
            }
        }
        self.dirty = true;
    }

    /// CatalogBehaviour.GetModOfSpawnable
    pub fn get_mod_of_spawnable(&self, name: &str) -> Option<&ModInfo> {
        self.local.spawnable_by_mod.get(name)
    }

    /// CatalogBehaviour.HideContraptionOutline: the key timer back to 0 and the outline hidden.
    pub fn hide_contraption_outline(&mut self) {
        self.local.seconds_spawn_key_held = 0.0;
        self.local.outline = None;
        super::uifn("CatalogBehaviour.HideContraptionOutline", || "False".into());
    }

    /// CatalogBehaviour.get_SelectedCategory: its name.
    pub fn selected_category(&self) -> String {
        let n = super::ui_text(self.catalog.categories.get(self.category).map_or("", |c| c.name.as_str()));
        super::uifn("CatalogBehaviour.get_SelectedCategory", || n.clone());
        n
    }

    /// CatalogBehaviour.ShowContraptionOutline: with a contraption selected, the outline file of its metadata read and shown, mirrored when `flipped`.
    /// A missing file or a failed read is logged and shows nothing.
    pub fn show_contraption_outline(&mut self, flipped: bool) {
        if self.local.selected.is_none() {
            return;
        }
        let path = self.local.meta.as_ref().and_then(|m| m.PathToOutlineFile.clone());
        let Some(path) = path.filter(|p| !p.as_os_str().is_empty() && p.exists()) else {
            eprintln!("Path to outline file does not exist: {:?}", self.local.meta.as_ref().and_then(|m| m.PathToOutlineFile.clone()));
            return;
        };
        match contraption::load_outline(&path) {
            Ok(outline) => self.local.outline = Some((outline, if flipped { -1.0 } else { 1.0 })),
            Err(e) => eprintln!("Failed to load contraption outline: {e}"),
        }
        // The outline's activeSelf and localScale.x.
        if let Some((_, x)) = &self.local.outline {
            let x = *x;
            super::uifn("CatalogBehaviour.ShowContraptionOutline", || format!("True {}", crate::game::menu::cs_float(x)));
        }
    }

    /// CatalogBehaviour.Update's outline half, with the catalog usable (no pause menu, dialog or UI block, something selected): a spawn key going down
    /// restarts the timer, holding one for the threshold with a contraption selected shows the outline (mirrored for spawnLeft), and releasing either
    /// hides it. `right` and `left` are InputSystem's down, held and up of spawnRight and spawnLeft, `dt` Time.unscaledDeltaTime.
    pub fn catalog_update(&mut self, right: (bool, bool, bool), left: (bool, bool, bool), dt: f32) {
        if self.item.is_none() && self.local.selected.is_none() {
            return;
        }
        if right.0 || left.0 {
            self.local.seconds_spawn_key_held = 0.0;
        }
        if self.local.seconds_spawn_key_held < SPAWN_KEY_HELD_THRESHOLD && self.local.selected.is_some() {
            for (held, flipped) in [(right.1, false), (!right.1 && left.1, true)] {
                if held {
                    self.local.seconds_spawn_key_held += dt;
                    if self.local.seconds_spawn_key_held >= SPAWN_KEY_HELD_THRESHOLD {
                        self.show_contraption_outline(flipped);
                    }
                    break;
                }
            }
        }
        if right.2 || left.2 {
            self.hide_contraption_outline();
        }
    }
}

/// CatalogBehaviour.CreateCategoryContainer, as CreateCategoryButtons uses it: the categories after the first 9 go in the dropdown's containers, each new
/// container (the first is made up front) made when the running count reaches a multiple of 11. The categories of each container, by index in the
/// catalog (the dropdown's own widget is not rewritten, so `view::catalog` stops before it is needed).
pub fn category_containers(category_count: usize) -> Vec<Vec<usize>> {
    const PREV_CATEGORY_COUNT: usize = 9;
    let mut containers: Vec<Vec<usize>> = vec![Vec::new()];
    let mut num = 0;
    for j in PREV_CATEGORY_COUNT..category_count {
        num += 1;
        if num % 11 == 0 {
            containers.push(Vec::new());
        }
        containers.last_mut().unwrap().push(j);
    }
    containers
}

/// ItemButtonBehaviour: one catalog button, a spawnable (`Item`) or a saved contraption (`Local`).
pub struct ItemButton<'a> {
    pub Item: Option<&'a Item>,
    pub Local: Option<&'a ContraptionMetaData>,
    pub PublishedFileId: u64,
}

/// What ItemButtonBehaviour.Start sets on the button: the tooltip text and which of its child objects are active.
#[derive(Debug, PartialEq)]
pub struct ItemButtonStart {
    pub tooltip: String,
    pub incompatible: bool,
    pub remove: bool,
    pub publish: bool,
    pub workshop: bool,
    pub info: bool,
}

impl<'a> ItemButton<'a> {
    pub fn of(item: &'a Item) -> Self {
        ItemButton { Item: item.local.is_none().then_some(item), Local: item.local.as_ref(), PublishedFileId: item.local.as_ref().map_or(0, |m| m.PublishedFileID) }
    }

    /// ItemButtonBehaviour.Start: a spawnable's tooltip is its bold name and description with the contraption buttons off; a contraption's is its bold
    /// name and "Saved in version V" (and the yellow warning on a different version, which shows Incompatible), Remove and Info on, Publish while it has
    /// no PublishedFileId and Workshop once it has.
    pub fn start(&self) -> ItemButtonStart {
        if let Some(item) = self.Item {
            return ItemButtonStart { tooltip: format!("<b>{}</b>\n{}", item.name, item.description), incompatible: false, remove: false, publish: false, workshop: false, info: false };
        }
        let Some(local) = self.Local else {
            return ItemButtonStart { tooltip: String::new(), incompatible: false, remove: false, publish: false, workshop: false, info: false };
        };
        let mut text = format!("<b>{}</b>\nSaved in version {}", local.DisplayName.as_deref().unwrap_or(""), local.version());
        let incompatible = !local.IsCurrentVersion();
        if incompatible {
            text += crate::il!(str "ItemButtonBehaviour::Start" 5);
        }
        ItemButtonStart { tooltip: text, incompatible, remove: true, publish: self.PublishedFileId == 0, workshop: self.PublishedFileId != 0, info: true }
    }

    /// ItemButtonBehaviour.ShowInfoDialog: a Notification of the contraption's name, version, size, source and required mods (green when enabled, red
    /// when not; no mod is loaded), with an "Open in Explorer" button. Its SetWidth(clamp(preferred + 32, 670, pixelWidth - 150)) and
    /// SetHeight(clamp(preferred + 32, 400, pixelHeight - 50)) change a sizeDelta the box's ContentSizeFitter sets again at the next layout (the
    /// game shows the box at its content's size), so the box keeps the notification's. A button without a contraption logs an error and shows nothing.
    pub fn show_info_dialog(&self, pixels: [f32; 2]) -> Option<DialogView> {
        let Some(local) = self.Local else {
            eprintln!("{}", crate::il!(str "ItemButtonBehaviour::ShowInfoDialog" 0));
            return None;
        };
        let mut text = String::new();
        let line = |s: &mut String, l: &str| {
            s.push_str(l);
            s.push_str(super::data::new_line());
        };
        line(
            &mut text,
            &format!(
                "<b><size=110%>{}</size></b> <alpha=#5A><size=80%>for {}\n{}<alpha=#FF><size=100%>",
                local.DisplayName.as_deref().unwrap_or(""),
                local.version(),
                crate::game::utils::get_formatted_byte_string(local.GetSizeOnDisk())
            ),
        );
        if !local.IsCurrentVersion() {
            line(&mut text, "Outdated");
        }
        line(&mut text, if self.PublishedFileId == 0 { crate::il!(str "ItemButtonBehaviour::ShowInfoDialog" 6) } else { crate::il!(str "ItemButtonBehaviour::ShowInfoDialog" 7) });
        if local.RequiresMods() {
            line(&mut text, crate::il!(str "ItemButtonBehaviour::ShowInfoDialog" 8));
            for m in local.RequiredMods.iter().flatten() {
                // ModLoaderRegistry.Mods is empty: no mod is enabled.
                text.push_str("<color=#f42d2d>");
                line(&mut text, &format!(" - {}", m.Name.as_deref().unwrap_or("")));
                text.push_str("</color>");
            }
        } else {
            line(&mut text, crate::il!(str "ItemButtonBehaviour::ShowInfoDialog" 13));
        }
        let mut dialog = DialogView::notification(text);
        let directory = local.PathToDataFile.as_ref().and_then(|p| p.parent()).map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
        dialog.buttons.push((crate::il!(str "ItemButtonBehaviour::ShowInfoDialog" 14).into(), false, Some(DialogRun::OpenFolder(directory))));
        // GetPreferredValues of the box's text (tmp::layout with the prefab's TextMeshProUGUI): Camera.main's pixel size
        // bounds the clamps.
        let [w, h] = super::view::dialog_preferred_values(&dialog.title);
        dialog.set_width((w + 32.0).clamp(670.0, pixels[0] - 150.0));
        dialog.set_height((h + 32.0).clamp(400.0, pixels[1] - 50.0));
        let state = format!("{} {}", dialog.buttons.iter().map(|b| b.0.as_str()).collect::<Vec<_>>().join("|"), super::ui_text(&dialog.title));
        super::uifn("ItemButtonBehaviour.ShowInfoDialog", || state);
        Some(dialog)
    }

    /// ItemButtonBehaviour.OpenWorkshop: the item's Steam Workshop page (Utils.OpenURL).
    pub fn workshop_url(&self) -> String {
        format!("https://steamcommunity.com/sharedfiles/filedetails/?id={}", self.PublishedFileId)
    }

    pub fn open_workshop(&self) {
        super::debug_info::open_url(&self.workshop_url());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(version: &str, published: u64) -> Item {
        let mut m = ContraptionMetaData::new("Car");
        m.Version = Some(version.into());
        m.PublishedFileID = published;
        Item { name: "Car".into(), description: String::new(), category: 0, order: "Car".into(), view: None, local: Some(m) }
    }

    #[test]
    fn dropdown_containers_split_every_eleven() {
        assert_eq!(category_containers(9), vec![Vec::<usize>::new()]);
        let c = category_containers(9 + 12);
        assert_eq!((c.len(), c[0].len(), c[1].len()), (2, 10, 11 - 10 + 1));
    }

    #[test]
    fn item_button_start_and_info() {
        // A save from the running version: "1.28" on 1.28.41, "1.28.57" on 1.28.57.
        let v = crate::game::contraption::current_version();
        let s = ItemButton::of(&saved(v, 0)).start();
        assert_eq!(s, ItemButtonStart { tooltip: format!("<b>Car</b>\nSaved in version {v}"), incompatible: false, remove: true, publish: true, workshop: false, info: true });
        let b = saved("1.0", 55);
        let s = ItemButton::of(&b).start();
        assert!(s.incompatible && !s.publish && s.workshop && s.tooltip.ends_with("This might cause issues."));
        let d = ItemButton::of(&b).show_info_dialog([1920.0, 1080.0]).unwrap();
        assert!(d.title.contains("Outdated") && d.title.contains(crate::il!(str "ItemButtonBehaviour::ShowInfoDialog" 7)) && d.title.contains(crate::il!(str "ItemButtonBehaviour::ShowInfoDialog" 13)));
        assert_eq!((d.width, d.height, d.buttons.len()), (Some(670.0), Some(400.0), 2));
        assert_eq!(ItemButton::of(&b).workshop_url(), "https://steamcommunity.com/sharedfiles/filedetails/?id=55");
        let spawnable = Item { local: None, ..saved("1.28", 0) };
        assert!(ItemButton::of(&spawnable).show_info_dialog([1920.0, 1080.0]).is_none());
        assert_eq!(ItemButton::of(&spawnable).start().tooltip, "<b>Car</b>\n");
    }

    #[test]
    fn selected_contraption_and_outline_timer() {
        let mut ui = Ui::new(Vec::new(), super::super::state::Mode::Menu, None, Some("Brick".into()));
        ui.set_contraption(Vec::new());
        assert!(ui.item.is_none() && ui.local.selected.is_some());
        // Held for 0.1 s with no outline file: logged, nothing shown; releasing resets the timer.
        ui.catalog_update((true, true, false), (false, false, false), 0.06);
        ui.catalog_update((false, true, false), (false, false, false), 0.06);
        assert!(ui.local.outline.is_none() && ui.local.seconds_spawn_key_held > 0.1);
        ui.catalog_update((false, false, true), (false, false, false), 0.016);
        assert_eq!(ui.local.seconds_spawn_key_held, 0.0);
        assert!(ui.get_mod_of_spawnable("Brick").is_none());
    }
}
