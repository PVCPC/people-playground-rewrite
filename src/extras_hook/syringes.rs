//! Extras syringes (not the game's): one per game liquid that has no syringe (ppg_extras::SYRINGES). Each is the Empty
//! Syringe's prefab under its own name; `liquid_of` tells the spawn which liquid fills it (src/game/extras_glue.rs),
//! and from then on it is an EmptySyringe whose GetLiquidID is that liquid: it starts full, refills while infinite,
//! and its fluid sprite shows the liquid's colour as every syringe's does.

use super::*;

/// The catalog item the syringes are built from.
pub const TEMPLATE: &str = "Empty Syringe";
/// The prefab cache keys (path ids no game object uses; the objects keep the template's ids).
const ROOT: i64 = BASE + 0x2_0000;

/// The syringes this game version has liquids for.
pub fn list() -> impl Iterator<Item = &'static x::Syringe> {
    x::SYRINGES.iter().filter(|s| !s.v45 || crate::assets::il::is_1_28_45())
}

pub(super) fn register(db: &mut AssetDb, by_name: &mut HashMap<String, ObjRef>) -> Result<(), String> {
    let template = *by_name.get(TEMPLATE).ok_or("extras: no Empty Syringe to build the syringes from")?;
    let empty = db.prefab_cached(template)?;
    for (i, s) in list().enumerate() {
        let mut p = (*empty).clone();
        p.objects[0].name = s.name.into();
        // A full syringe starts pushing (the Empty Syringe's is set to pull).
        for (_, c) in &mut p.objects[0].components {
            if let Component::Script { class, fields, .. } = c {
                if class == "EmptySyringe" {
                    set(fields, "PressureMode", Value::Int(0));
                }
            }
        }
        let root = ObjRef { file: template.file, path_id: ROOT + i as i64 };
        db.cache_prefab(root, p);
        by_name.insert(s.name.to_string(), root);
    }
    Ok(())
}

/// The liquid (a `LIQUIDS` index) an extras syringe's prefab holds, known by its root's name; None for any other prefab.
pub fn liquid_of(prefab: &Prefab) -> Option<usize> {
    let root = prefab.objects.first()?;
    if !root.components.iter().any(|(_, c)| matches!(c, Component::Script { class, .. } if class == "EmptySyringe")) {
        return None;
    }
    list().find(|s| s.name == root.name).map(|s| crate::game::circulation::liquid_of_class(s.liquid))
}

#[cfg(test)]
mod tests {
    use crate::assets::db::AssetDb;
    use crate::assets::mono::Value;
    use crate::game::container::Owner;
    use crate::game::Game;
    use crate::play::{new_game, step};

    fn frames(g: &mut Game, n: usize) {
        for _ in 0..n {
            step(g, 1.0 / 60.0).unwrap();
        }
    }

    /// Spawns catalog item `name` at `at`; its first physical (the root's when it has one).
    fn spawn(g: &mut Game, name: &str, at: [f32; 2]) -> usize {
        let mut db = g.assets.take().unwrap();
        let (inst, linked) = g.catalog_spawn(&mut db, name, [at[0], at[1], 0.0], false).unwrap_or_else(|e| panic!("{name}: {e}"));
        g.unsynced.push(inst);
        g.assets = Some(db);
        linked.physicals.iter().min_by_key(|(o, _)| **o).map(|(_, p)| *p).unwrap_or_else(|| panic!("{name}: no physical"))
    }

    fn syringe_of(g: &Game, p: usize) -> usize {
        g.syringes.iter().position(|s| s.Phys == p).expect("a syringe")
    }

    /// Saves the object at physical `p` (ObjectStates to bytes and back) and loads it at `at`: the name it was saved
    /// under, and the loaded root's physical.
    fn save_load(g: &mut Game, p: usize, at: [f32; 2]) -> (String, usize) {
        let states = g.object_states_of(&[p], [0.0; 3]);
        assert_eq!(states.len(), 1);
        let bytes = crate::game::contraption::serialise_states(&states).unwrap();
        let states = crate::game::contraption::deserialise_states(&bytes).unwrap();
        let first = g.physicals.len();
        g.paste_states(&states, [at[0], at[1], 0.0], false, "Paste");
        frames(g, 2);
        assert!(g.physicals.len() > first, "{}: nothing loaded", states[0].SpawnableAssetName);
        (states[0].SpawnableAssetName.clone(), first)
    }

    /// Every extras syringe spawns full of its liquid (and only it), refills, and saves and loads as itself.
    #[test]
    fn extras_syringes_spawn_full_and_round_trip() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut g = new_game(&game, "Default", None, None, 0).unwrap();
        let list: Vec<_> = super::list().collect();
        assert_eq!(list.len(), if crate::assets::il::is_1_28_45() { 31 } else { 28 });
        let made: Vec<usize> = list.iter().enumerate().map(|(k, s)| spawn(&mut g, s.name, [k as f32 * 0.5 - 7.0, 3.0])).collect();
        frames(&mut g, 3);
        for (s, &p) in list.iter().zip(&made) {
            let l = crate::game::circulation::liquid_of_class(s.liquid);
            let k = syringe_of(&g, p);
            assert_eq!(g.syringes[k].liquid, Some(l), "{}", s.name);
            let o = Owner::Syringe(k);
            assert!((g.get_amount(o, l) - 1.4).abs() < 1e-3, "{}: {} of its liquid", s.name, g.get_amount(o, l));
            let (name, loaded) = save_load(&mut g, p, [0.0, 6.0]);
            assert_eq!(name, s.name);
            let k2 = syringe_of(&g, loaded);
            assert_eq!(g.syringes[k2].liquid, Some(l), "{} loaded", s.name);
            assert!(g.get_amount(Owner::Syringe(k2), l) > 1.0, "{} loaded holds {}", s.name, g.get_amount(Owner::Syringe(k2), l));
        }
    }

    /// Each extras syringe lodged in a Human pushes its liquid into the Human's circulation.
    #[test]
    fn extras_syringes_inject_into_a_human() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        for s in super::list() {
            let mut g = new_game(&game, "Default", None, None, 0).unwrap();
            let l = crate::game::circulation::liquid_of_class(s.liquid);
            let first = g.limbs.len();
            spawn(&mut g, "Human", [0.0, 3.0]);
            let p = spawn(&mut g, s.name, [3.0, 3.0]);
            frames(&mut g, 2);
            // The upper body (its own circulation, not the head: some liquids act on the brain at once).
            let limb = (first..g.limbs.len()).find(|&b| g.physicals[g.limbs[b].PhysicalBehaviour].name.contains("UpperBody")).expect("an upper body");
            let k = syringe_of(&g, p);
            g.syringe_lodged(k, g.limbs[limb].PhysicalBehaviour);
            let total = |g: &Game| -> f32 { (first..g.limbs.len()).map(|b| g.get_amount(Owner::Circulation(b), l)).sum() };
            // Blood is in the Human already: more of it counts.
            let before = total(&g);
            let mut seen = before;
            for _ in 0..30 {
                frames(&mut g, 1);
                seen = seen.max(total(&g));
            }
            // Dulcamarus leaves the blood at once (its OnEnterLimb gives the person a DulcamarusBehaviour).
            assert!(seen > before + 1e-4 || s.liquid == "Dulcamarus", "{}: no {} in the Human", s.name, s.liquid);
        }
    }

    /// The Hidden items tab: last, every SpawnableAsset the game's catalog hides (VisibleInCatalog false or Locked);
    /// each spawns, runs and saves and loads.
    #[test]
    fn hidden_items_tab() {
        let Ok(game) = crate::game_assets::find_game(None) else { return };
        let mut db = AssetDb::open(&game).unwrap();
        let catalog = crate::ui::data::catalog(&mut db).unwrap();
        let tab = catalog.categories.len() - 1;
        assert_eq!(catalog.categories[tab].name, ppg_extras::HIDDEN_CATEGORY);
        assert!(catalog.categories[tab].icon.is_some());
        // The game's categories stay as they were.
        assert_eq!(catalog.categories[..tab].iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Entities", "Melee", "Firearms", "Explosives", "Vehicles", "Machinery", "Chemistry", "Misc.", "Local"]);
        let mut want = Vec::new();
        for r in db.resources_in("SpawnableAssets", 114).unwrap() {
            let (_, mb, v) = db.mono(r).unwrap().unwrap();
            if !v.get("VisibleInCatalog").and_then(Value::bool).unwrap_or(true) || v.get("Locked").and_then(Value::bool).unwrap_or(false) {
                want.push(mb.name);
            }
        }
        let mut have: Vec<String> = catalog.items.iter().filter(|i| i.category == tab).map(|i| i.name.clone()).collect();
        println!("HIDDEN TAB {}", have.join(" | "));
        have.sort();
        want.sort();
        assert_eq!(have, want);
        for name in ["Normal Sized Gun", "Old Tank", "Water Detector", "Liquid Level Detector", "Bell C4", "Gear 32"] {
            assert!(have.iter().any(|h| h == name), "{name} missing");
        }
        if crate::assets::il::is_1_28_45() {
            assert!(have.iter().any(|h| h == "Old Thruster"));
        }
        let mut g = new_game(&game, "Default", None, None, 0).unwrap();
        for (k, name) in have.iter().enumerate() {
            let p = spawn(&mut g, name, [(k % 8) as f32 * 3.0 - 10.0, 3.0 + (k / 8) as f32 * 4.0]);
            frames(&mut g, 2);
            let (saved, _) = save_load(&mut g, p, [(k % 8) as f32 * 3.0 - 10.0, 20.0 + (k / 8) as f32 * 4.0]);
            assert_eq!(&saved, name);
        }
        frames(&mut g, 30);
    }
}
