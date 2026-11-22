//! --play <recording>: the rewritten game in a window, starting from the game's own state in a PPG_DUMP recording
//! (its first FixedUpdate snapshot with the b2 world) and running Unity's player loop from there.
//! --play-map <map>: the same, starting from LoadScene("Main") with that map, all from the game's data.

use crate::b2::world::Shape;
use crate::game::{dump, Game, Prefs};
use crate::game_assets::HumanRig;
use bevy::prelude::*;
use crate::assets::db::AssetDb;
use std::path::{Path, PathBuf};

#[derive(Resource)]
pub(crate) struct Play {
    pub game: Game,
    /// Set when a frame panicked (an unrewritten feature): the world stays frozen as it was.
    pub stopped: Option<String>,
    /// How many limbs, physicals and renderers already have a sprite.
    done: (usize, usize, usize),
}

impl Play {
    /// The Main scene starting: NotificationControllerBehaviour.Start shows its startingHints (the scene's one,
    /// "Welcome to the playground!") before the backlog.
    pub fn new(mut game: Game) -> Self {
        game.notifications.insert(0, "Welcome to the playground!".into());
        Self { game, stopped: None, done: (0, 0, 0) }
    }
}

/// A limb's sprite, drawn at its PhysicalBehaviour's Transform.
/// An object of the Main scene: destroyed with it when it unloads (a map load or the menu), as SceneManager.LoadScene
/// destroys every object of the scene it replaces.
#[derive(Component, Default)]
pub(crate) struct MainScene;

#[derive(Component)]
#[require(MainScene)]
pub(crate) struct LimbSprite(pub usize);

/// The `Skin::version` of a limb whose sprite, flesh and bone textures are drawn.
#[derive(Component)]
pub(crate) struct Swapped(u32);

/// Where the game starts.
pub enum Start {
    Recording(PathBuf),
    /// The installed game's folder, the Map asset's name and the catalog item selected (if any).
    /// The game, the map, the --item and the --target.
    Map(PathBuf, String, Option<String>, Option<String>),
    /// The main menu, with the installed game's folder.
    Menu(PathBuf),
}

/// `prefs` replace config.json's (the windowed game's settings); None for the headless runs.
fn load(start: &Start, rig: &HumanRig, prefs: Option<Prefs>) -> Result<Option<Game>, String> {
    let recording = match start {
        Start::Recording(recording) => recording,
        Start::Map(game, map, item, _) => return new_game(game, map, item.as_deref(), prefs, 0).map(Some),
        Start::Menu(_) => return Ok(None),
    };
    let start = dump::snapshots(recording)?
        .find(|s| s.pt == "FS" && s.of("b2World").next().is_some())
        .ok_or("--play needs a recording with the b2 world (dump12 on)")?;
    let mut g = Game::from_snapshot(&start, rig)?;
    g.load_layers(&dump::header(recording)?);
    Ok(Some(g))
}

/// A panic in `f` (an unrewritten feature) comes back as its message.
pub(crate) fn guarded<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|panic| Err(panic_message(panic)))
}

fn panic_message(panic: Box<dyn std::any::Any + Send>) -> String {
    panic.downcast_ref::<String>().cloned().or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()
}

/// SceneManager.LoadScene("Main") with the main menu's map choice: level1's managers and natives, then the
/// MapLoaderBehaviour's Awake. Its behaviours' Starts run in the first frame.
/// `seed` is UnityEngine.Random's startup seed.
/// The windowed game preloads the catalog's prefabs and textures at load; the headless tests do not.
pub static PRELOAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn new_game(game: &Path, map: &str, item: Option<&str>, prefs: Option<Prefs>, seed: i32) -> Result<Game, String> {
    let mut job = LoadJob::new(game, map, item, prefs, seed);
    loop {
        if let Some(g) = job.advance()? {
            return Ok(g);
        }
    }
}

/// `new_game` one step at a time, so the windowed game can draw the loading bar between steps (SceneManager.LoadSceneAsync).
/// The steps run in the same order either way, so the loaded game is the same.
pub(crate) struct LoadJob {
    game: std::path::PathBuf,
    map: String,
    item: Option<String>,
    prefs: Option<Prefs>,
    seed: i32,
    db: Option<AssetDb>,
    g: Option<Game>,
    step: u8,
    progress: f32,
}

impl LoadJob {
    pub fn new(game: &Path, map: &str, item: Option<&str>, prefs: Option<Prefs>, seed: i32) -> Self {
        Self { game: game.into(), map: map.into(), item: item.map(str::to_string), prefs, seed, db: None, g: None, step: 0, progress: 0.0 }
    }

    /// AsyncOperation.progress after the steps done so far. LoadSceneOperation.Perform (UnityPlayer 0x18077c600) reads
    /// the objects the scene's PreloadData (sharedassets1, file ID 1) lists, then all of sharedassets1, then level1's,
    /// and before each object sets progress = read * 0.9 / total (PersistentManager::LoadObjectsThreaded and
    /// LoadFileCompletelyThreaded), where the total starts as PreloadData's count plus level1's and grows by
    /// sharedassets1's when that file starts. It sets 0.9 at the end; the main thread's activation (the Awakes) keeps
    /// it there until isDone. Step 0 is the reads up to level1, step 1 builds level1's objects, the rest is activation.
    pub fn progress(&self) -> f32 {
        if self.step >= 2 { 0.9 } else { self.progress }
    }

    /// The next step; the game after the last.
    pub fn advance(&mut self) -> Result<Option<Game>, String> {
        let missing = "LoadJob: a step ran out of order";
        match self.step {
            0 => {
                let mut db = AssetDb::open(&self.game)?;
                self.progress = scene_read_progress(&db)?;
                let mut g = Game::load_main(&mut db, &self.game)?;
                g.random = crate::unity::Random::new(self.seed);
                // The heap addresses behind GetHashCode (which reseeds Random in PhysicalBehaviour.Start) differ per session too.
                if self.seed != 0 {
                    g.heap_base = Some(0x0100_0000 + (((self.seed as u32) as u64 & 0xf_ffff) << 11));
                }
                if let Some(prefs) = self.prefs.take() {
                    g.prefs = prefs;
                }
                // DiskLogger.Awake (Main's), among level1's Awakes.
                g.disk_logger = Some(crate::game::disk_logger::DiskLogger::awake(&crate::game::disk_logger::user_data_dir(), "Main"));
                (self.db, self.g) = (Some(db), Some(g));
            }
            1 => self.g.as_mut().ok_or(missing)?.load_scene(self.db.as_mut().ok_or(missing)?)?,
            2 => {
                // StatLoader.Start (execution order 30), then Global.Start's stats (order 50).
                if let Some(e) = crate::game::stats::load_from_file() {
                    // Debug.LogException of what left StatLoader.Start.
                    println!("exception	{e}");
                    self.g.as_mut().ok_or(missing)?.log_message_received(&e, crate::game::menu::LogType::Exception);
                }
                crate::game::stats::global_start();
                self.g.as_mut().ok_or(missing)?.map_loader_awake(self.db.as_mut().ok_or(missing)?, &self.map)?;
            }
            3 => {
                if PRELOAD.load(std::sync::atomic::Ordering::Relaxed) {
                    Game::preload_catalog(self.db.as_mut().ok_or(missing)?);
                }
            }
            _ => {
                let mut g = self.g.take().ok_or(missing)?;
                g.assets = Some(Box::new(self.db.take().ok_or(missing)?));
                g.catalog_selected = self.item.take();
                return Ok(Some(g));
            }
        }
        self.step += 1;
        Ok(None)
    }
}

/// LoadSceneOperation.Perform's progress once PreloadData's objects and sharedassets1's are read (LoadJob::progress).
fn scene_read_progress(db: &AssetDb) -> Result<f32, String> {
    let file = |n: &str| db.file_index(n).ok_or_else(|| format!("no {n}"));
    let (shared, level) = (file("sharedassets1.assets")?, file("level1")?);
    let preload = crate::assets::db::ObjRef { file: shared, path_id: 1 };
    if db.class(preload) != 150 {
        return Err("sharedassets1's file ID 1 is no PreloadData".into());
    }
    // PreloadData: m_Name, then m_Assets.
    let r = &mut crate::assets::serialized::Reader::new(db.data(preload));
    r.string()?;
    let p = r.i32()? as f32;
    // GetFileIDsForLoadingScene leaves out GlobalGameManagers, which a level file has none of.
    let (a, s) = (db.files[shared].objects.len() as f32, db.files[level].objects.len() as f32);
    Ok((p + a) * 0.9 / (p + s + a))
}

/// --play-test: 30 s of 60 fps frames without a window; prints where each limb ends up. A --play-map-test with
/// --item spawns the item at the map's center on frame 30 and a flipped one on frame 60. With a --target, the target
/// spawns 3 m right of the center, 1.2 m above the ground there, on frame 30 and the item 0.6 m above the ground
/// below the center on frame 60, activated ("activateDirect" with the mouse on its first object's center of mass)
/// on frame 64.
pub fn test(start: &Start, rig: &HumanRig) -> Result<(), String> {
    let mut g = load(start, rig, None)?.ok_or("the menu has no headless run")?;
    if let Ok(c) = std::env::var("PP_STOP_PROBE") {
        if c == "@prefabs" {
            g.stop_probe_prefabs();
            return Ok(());
        }
        g.stop_probe(&c.split(',').map(str::to_string).collect::<Vec<_>>());
        return Ok(());
    }
    let target = match start {
        Start::Map(_, _, _, target) => target.clone(),
        _ => None,
    };
    let item = g.catalog_selected.clone();
    // PP_HOLD=N (with a --target): the item is dragged from frame 62 and "activateDirect" is held for N frames from 64.
    let hold: i32 = std::env::var("PP_HOLD").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut ground = 0.0;
    let mut item_physical = 0;
    // PP_RENDER: where the item's renderers, lines and texts start in `Game::renderers`, `lines` and `world_texts`.
    let mut item_from = (0, 0, 0);
    // PP_CLICK: the menu buttons still to click, one a frame (object name, identity, actions).
    let mut clicks: Vec<(usize, &str, Vec<crate::game::menu::MenuAction>)> = Vec::new();
    let mut target_physical = 0;
    for n in 0..1800 {
        if let Some(target) = &target {
            if n == 29 {
                target_physical = g.physicals.len();
                g.catalog_selected = Some(target.clone());
                let c = g.BoundingBox.center;
                ground = g.raycast_first(crate::b2::math::Vec2::new(c[0], c[1]), [0.0, -1.0], 1.0e4).ok_or("--target: no ground below the map's center")?.point.y;
            } else if n == 59 {
                g.catalog_selected = item.clone();
                item_physical = g.physicals.len();
                item_from = (g.renderers.len(), g.lines.len(), g.world_texts.len());
            }
            // PP_USE_AT: the frame the item is used on (default 64).
            let use_at: i32 = std::env::var("PP_USE_AT").ok().and_then(|v| v.parse().ok()).unwrap_or(64);
            for (key, at) in [("spawnRight", &[30, 60][..]), ("activateDirect", &[use_at][..])] {
                let down = at.contains(&(n + 1));
                let (held, up) = match key {
                    "activateDirect" if hold > 0 => ((63..63 + hold).contains(&n), n == 63 + hold),
                    _ => (down, at.contains(&n)),
                };
                g.input.controls.insert(key.into(), crate::game::tool::Control { down, held, up });
            }
            if hold > 0 {
                g.input.controls.insert("drag".into(), crate::game::tool::Control { down: n == 61, held: n >= 61, up: false });
            }
            g.input.mouse = match n {
                ..59 => [g.BoundingBox.center[0] + 3.0, ground + 1.2, 0.0],
                ..61 => [g.BoundingBox.center[0], ground + 0.6, 0.0],
                _ => {
                    let p = g.physicals.get(item_physical).ok_or("--target: the item has no physical object")?;
                    // Once the item is gone the mouse stays where it was.
                    match p.destroyed {
                        true => g.input.mouse,
                        false => {
                            let c = g.world.body(p.rigidbody.body).sweep.c;
                            [c.x, c.y, 0.0]
                        }
                    }
                }
            };
            // PP_RENDER: the reference was taken with nothing hovered or selected (no selection outline), so the mouse leaves the item
            // for the frames before the dump. Global.MousePosition follows the mouse after ToolControllerBehaviour.Update, so the
            // hover the frame-62 tool update sees is frame 61's mouse: it leaves from 61 on.
            if (61..64).contains(&n) && std::env::var("PP_RENDER").is_ok() {
                g.input.mouse = [1.0e5, 1.0e5, 0.0];
                // The reference spawns without the catalog, so nothing is selected; the drag here leaves the item selected.
                g.clear_selection();
            }
        } else if g.catalog_selected.is_some() {
            // With a catalog item selected: "spawnRight" released at the map's center on frame 30, "spawnLeft" 3 m
            // to the right on frame 60.
            for (key, at, dx) in [("spawnRight", 30, 0.0), ("spawnLeft", 60, 3.0)] {
                g.input.controls.insert(key.into(), crate::game::tool::Control { down: n == at - 1, held: n == at - 1, up: n == at });
                if n == at {
                    g.input.mouse = [g.BoundingBox.center[0] + dx, g.BoundingBox.center[1], 0.0];
                }
            }
        }
        if target.is_none() && n == 30 {
            item_physical = g.physicals.len();
            item_from = (g.renderers.len(), g.lines.len(), g.world_texts.len());
        }
        // PP_DETAIL=N: the detail view is toggled on frame N (Global.ToggleLimbStatus).
        let detail = std::env::var("PP_DETAIL").ok().and_then(|v| v.parse().ok());
        if detail == Some(n) {
            g.toggle_limb_status();
            eprintln!("detail view {} on frame {n}", g.ShowLimbStatus);
        }
        // PP_STRESS=shoot|explode|burn|heat|cold|charge: on frame 400 that happens to every physical object of the item
        // (a shot of 25, an explosion on it, ignition, 1200 or -250 degrees, 1000 charge); the run goes on to frame 900.
        if let (Ok(kind), true) = (std::env::var("PP_STRESS"), n == 400) {
            for q in item_physical..g.physicals.len() {
                let c = g.world.body(g.physicals[q].rigidbody.body).sweep.c;
                match kind.as_str() {
                    "shoot" => {
                        if let Some(&collider) = g.physicals[q].all_colliders.first() {
                            g.send_shot_for_test(collider, q, [c.x, c.y], 25.0);
                        }
                    }
                    "explode" => g.create_explosion_with_water(false, crate::game::explosion::ExplosionParameters {
                        FragmentationRayCount: 32,
                        Position: [c.x, c.y, 0.0],
                        Range: 6.0,
                        FragmentForce: 12.0,
                        CreateParticlesAndSound: true,
                        EffectSize: crate::game::explosion::EffectSize::Medium,
                        DismemberChance: 0.5,
                        BallisticShrapnelCount: 0,
                    }),
                    "burn" => g.ignite(q, true),
                    "heat" => g.physicals[q].Temperature = 1200.0,
                    "cold" => g.physicals[q].Temperature = -250.0,
                    "charge" => g.physicals[q].charge = 1000.0,
                    other => panic!("PP_STRESS={other}"),
                }
            }
        }
        // PP_HUMAN=drop|use|crush|freeze (with --target Human): on frame 400 the item moves onto the human (above the head
        // for drop, over the torso otherwise); crush also crushes every limb, freeze freezes them. PP_USE_AT=404 uses it there.
        if let (Ok(kind), true) = (std::env::var("PP_HUMAN"), n == 400) {
            let c = g.world.body(g.physicals[target_physical].rigidbody.body).sweep.c;
            let ic = g.world.body(g.physicals[item_physical].rigidbody.body).sweep.c;
            let lift = if kind == "drop" { 2.0 } else { 0.0 };
            let delta = [c.x - ic.x, c.y - ic.y + lift, 0.0];
            for q in item_physical..g.physicals.len() {
                g.move_physical_for_test(q, delta);
            }
            let limbs: Vec<usize> = (0..g.limbs.len()).filter(|&l| (target_physical..item_physical).contains(&g.limbs[l].PhysicalBehaviour)).collect();
            match kind.as_str() {
                "drop" | "use" => {}
                "crush" => limbs.iter().for_each(|&l| g.crush_limb_for_test(l)),
                "freeze" => limbs.iter().for_each(|&l| g.limbs[l].Frozen = true),
                other => panic!("PP_HUMAN={other}"),
            }
        }
        // PP_RENDER: the item's renderer state at the same frame (tools/pc/render.py compares it with the game's,
        // reference-probe/game/render.txt): each SpriteRenderer's sprite, shown, colour, sorting layer and order; each
        // TextMeshPro's shown and text; each LineRenderer's shown and point count.
        if n == if target.is_some() { 63 } else { 45 } && std::env::var("PP_RENDER").is_ok() {
            render_dump(&g, item_physical, item_from);
        }
        if n == std::env::var("PP_DD_FRAME").ok().and_then(|v| v.parse().ok()).unwrap_or(if target.is_some() { 63 } else { 45 }) && (std::env::var("PP_DRAWDIFF").is_ok() || std::env::var("PP_MAPDUMP").is_ok()) {
            let name = g.catalog_selected.clone().or(item.clone()).unwrap_or_default();
            drawdiff(&mut g, &name, item_physical, item_from);
        }
        // PP_MENUS: the item's ContextMenuOptions buttons once it has started (tools/menus.py compares them with the
        // game's, reference-probe/game/menus.txt).
        if n == if target.is_some() { 63 } else { 45 } && std::env::var("PP_MENUS").is_ok() {
            for q in item_physical..g.physicals.len() {
                for b in &g.physicals[q].menu {
                    let cond = g.menu_condition(b.condition);
                    println!("menu	{}	{}	{}	{}	{}	{}", g.physicals[q].name, b.Identity, g.menu_label(b.label).replace('\n', "\\n"), b.Description, b.LabelWhenMultipleAreSelected, if cond { "True" } else { "False" });
                    // PP_CLICK: every shown button that needs no dialog gets clicked, in menu order.
                    if cond && std::env::var("PP_CLICK").is_ok() && !b.actions.iter().any(|a| a.needs_ui()) {
                        clicks.push((q, b.Identity, b.actions.clone()));
                    }
                }
            }
            clicks.reverse();
        } else if let Some((q, id, actions)) = clicks.pop() {
            // A destroyed object closes its menu (an exploded bomb): the rest are not clicked.
            if g.physicals[q].destroyed {
                clicks.clear();
            } else {
                println!("click	{}	{id}", g.physicals[q].name);
                if !g.context_menu_click(&actions) {
                    println!("click threw");
                }
                // A number entry box: Apply with the text it opened with.
                if g.entry.is_some() {
                    // PP_ENTRY: the text Apply is given instead (a temperature or an angle, say).
                    if let (Ok(text), Some(e)) = (std::env::var("PP_ENTRY"), g.entry.as_mut()) {
                        e.text = text;
                    }
                    g.entry_apply();
                }
                // The colour picker: Apply with the colour it opened with.
                if g.colour.is_some() {
                    g.colour_apply();
                }
                // The piano: Apply with the bell's own clip.
                g.piano_apply();
                // Key entry boxes: Apply with the key they opened with.
                for b in std::mem::take(&mut g.key_boxes) {
                    g.key_triggers[b.trigger].KeyCode = b.key;
                    g.key_trigger_update_display(b.trigger);
                }
            }
        }
        // PP_WIRE=<wire tool> (with a --target): on frame W (PP_WIRE_AT, default 900) that tool is chosen, and "drag" draws a
        // wire from the item's center of mass (frame W+1) to the target's first physical object (released on W+6). An item
        // or target that exploded or vanished before then ends the hook with a note instead of a panic.
        if let (Ok(tool), Some(_)) = (std::env::var("PP_WIRE"), &target) {
            let w: i32 = std::env::var("PP_WIRE_AT").ok().and_then(|v| v.parse().ok()).unwrap_or(900);
            let n = n as i32;
            let center = |g: &crate::game::Game, p: usize| {
                let ph = g.physicals.get(p).filter(|ph| !ph.destroyed)?;
                let c = g.world.body(ph.rigidbody.body).sweep.c;
                Some([c.x, c.y, 0.0])
            };
            if n == w {
                g.set_tool_by_type_name(&tool);
            }
            if (w + 1..=w + 6).contains(&n) {
                let at = if n == w + 1 { center(&g, item_physical) } else { center(&g, target_physical) };
                match at {
                    None => println!("wire frame {n}: the item or the target is gone"),
                    Some(at) => {
                        g.input.mouse = at;
                        // Input.mousePosition: 100 pixels a metre from the world origin.
                        let m = g.input.mouse;
                        g.input.screen = Some(crate::game::tool::Screen { mouse: [m[0] * 100.0, m[1] * 100.0], origin: [0.0; 2], scale: 0.01, view: None });
                        g.input.controls.insert("drag".into(), crate::game::tool::Control { down: n == w + 1, held: n < w + 6, up: n == w + 6 });
                    }
                }
            }
            if n % 100 == 0 && n >= w {
                println!("wire frame {n}: wires={} item charge={} target charge={}", g.blood_wires.len(), g.physicals[item_physical].charge, g.physicals[target_physical].charge);
            }
        }
        // PP_ATTACH=<attachment item>[,<point index>]: on frame 900 that item is dropped on the gun's attachment point.
        if let Ok(spec) = std::env::var("PP_ATTACH") {
            let mut it = spec.split(',');
            let name = it.next().unwrap();
            let pt: usize = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            if n == 899 {
                g.catalog_selected = Some(name.to_string());
                g.input.controls.insert("spawnRight".into(), crate::game::tool::Control { down: true, held: true, up: false });
            }
            if n == 900 {
                g.input.controls.insert("spawnRight".into(), crate::game::tool::Control { down: false, held: false, up: true });
                let at = g.attachment_point_position(pt);
                g.input.mouse = at;
                println!("attachment point {pt} at {at:?}");
            }
            // The gun fires with the attachment on it (frames 1000, 1040, 1080), then the attachment is activated: it disconnects.
            let (gun, att) = if n >= 901 { (g.firearms[0].physical.unwrap(), g.firearm_attachments[0].physical.unwrap()) } else { (0, 0) };
            let report = |g: &crate::game::Game, what: &str| {
                let (a, f) = (&g.firearm_attachments[0], &g.firearms[0]);
                let em = f.emitter.as_ref();
                println!(
                    "{what}: connected={} listeners={:?} parent={} driven={} charge={} gun temp={} applyCharge={} inaccuracy={:?} explosive={:?} laser={:?} on fire={} burn sum={} limb health sum={}",
                    a.Connected, f.attachments, g.physicals[att].parent.is_some(), g.physicals[att].driven_by.is_some(), g.physicals[gun].charge, g.physicals[gun].Temperature, f.ApplyCharge, f.InaccuracyMultipliers,
                    em.map(|e| (e.ExplosiveRound, e.ExplodeAtHitPoint)), a.laser.map(|l| g.lasers[l].Activated),
                    g.physicals.iter().filter(|p| p.OnFire).count(), g.physicals.iter().map(|p| p.BurnProgress).sum::<f32>(),
                    g.limbs.iter().map(|l| l.Health).sum::<f32>()
                );
            };
            match n {
                ..901 => {}
                960 | 1120 | 1260 | 1400 => report(&g, &format!("frame {n}")),
                1000 | 1040 | 1080 => g.activate_for_test(gun),
                1200 if std::env::var("PP_ATTACH_DESTROY").is_err() => g.activate_for_test(att),
                // PP_ATTACH_DESTROY=gun|att: that object is destroyed while connected, instead of the attachment being activated.
                1100 if std::env::var("PP_ATTACH_DESTROY").is_ok() => {
                    let victim = if std::env::var("PP_ATTACH_DESTROY").is_ok_and(|v| v == "gun") { gun } else { att };
                    g.destroy_object(g.physicals[victim].id);
                }
                _ => {}
            }
        }
        // PP_CONTRAPTION=<.jaap> (--play-map-test Default): the file is read where it lies and loaded with ContraptionSerialiser's path
        // (Ceras bytes -> ObjectStates -> paste) at the map's center on frame 30; after 10 s of play (frame 660) the loaded objects are deleted and
        // undone, copied, pasted and undone, and saved (the serialise, deserialise and paste the save dialog uses; the folder write goes to PP_CONTRAPTION_SAVE).
        // An action that panics prints ABUSE-CAUGHT <action> after the panic message and the run goes on.
        if let Ok(file) = std::env::var("PP_CONTRAPTION") {
            if n == 30 {
                item_physical = g.physicals.len();
                let mut meta = crate::game::contraption::ContraptionMetaData::new(Path::new(&file).file_stem().and_then(|s| s.to_str()).unwrap_or("contraption"));
                meta.PathToDataFile = Some(PathBuf::from(&file));
                meta.Version = Some(crate::game::contraption::current_version().into());
                let mut messages = crate::game::contraption::LoadMessages::default();
                let states = crate::game::contraption::load_contraption(&mut meta, &mut messages)?;
                eprintln!("CONTRAPTION {} states, notifications {:?}, dialogs {:?}", states.len(), messages.notifications, messages.dialogs);
                if std::env::var("PP_CONTRAPTION_LIST").is_ok() {
                    for st in &states {
                        if std::env::var("PP_CONTRAPTION_LIST").as_deref() == Ok("protos") {
                            for m in st.RootComponentData.iter().chain(st.ChildComponentData.iter().flat_map(|(_, v)| v.iter())) {
                                eprintln!("PROTO {} {:?}", st.SpawnableAssetName, m);
                            }
                        }
                        let kinds: Vec<&str> = st.RootComponentData.iter().chain(st.ChildComponentData.iter().flat_map(|(_, v)| v.iter())).map(|m| m.Type.as_str()).collect();
                        eprintln!("STATE {} {}", st.SpawnableAssetName, kinds.join(","));
                        for (k, v) in &st.ChildComponentData {
                            eprintln!("CHILD {} {} {}", st.SpawnableAssetName, k, v.iter().map(|m| m.Type.as_str()).collect::<Vec<_>>().join(","));
                        }
                    }
                    return Ok(());
                }
                let c = g.BoundingBox.center;
                g.paste_states(&states, [c[0], c[1], 0.0], false, "Load");
            }
            let item_range = item_physical..g.physicals.len();
            let act = |g: &mut crate::game::Game, a: &str| {
                eprintln!("ABUSE-PHASE {a}");
                let r = item_range.clone();
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.abuse_action(a, r))).is_err() {
                    eprintln!("ABUSE-CAUGHT {a}");
                }
            };
            match n {
                660 => act(&mut g, "copy_paste_undo"),
                720 => act(&mut g, "contraption"),
                780 => {
                    eprintln!("ABUSE-PHASE save");
                    let save = std::env::var("PP_CONTRAPTION_SAVE").unwrap_or_else(|_| "scratch/contraption-save".into());
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let live: Vec<usize> = (item_physical..g.physicals.len()).filter(|&p| !g.physicals[p].destroyed).collect();
                        let c = g.BoundingBox.center;
                        let states = g.object_states_of(&live, [c[0], c[1], 0.0]);
                        crate::game::contraption::save_contraption(Path::new(&save), "probe", &states)
                    }));
                    match r {
                        Ok(Ok(_)) => {}
                        Ok(Err(e)) => eprintln!("ABUSE-ERR save {e}"),
                        Err(_) => eprintln!("ABUSE-CAUGHT save"),
                    }
                }
                840 => act(&mut g, "delete_undo"),
                900 => break,
                _ => {}
            }
        }
        // PP_ABUSE (with a --target): from frame 400 the item is abused with the paths real play uses (src/game/abuse.rs for the
        // damage, copy, save and delete ones; the tool controller for the pins, wires and powers drawn from the item to the human).
        // An action that panics prints ABUSE-CAUGHT <action> after the panic message and the run goes on. PP_ABUSE_SKIP=a,b skips actions.
        if std::env::var("PP_ABUSE").is_ok() && target.is_some() && n >= 400 {
            let skip = std::env::var("PP_ABUSE_SKIP").unwrap_or_default();
            let skipped = |a: &str| skip.split(',').any(|x| x == a);
            let n = n as usize;
            let item_range = item_physical..g.physicals.len();
            let mut act = |g: &mut crate::game::Game, a: &str| {
                if skipped(a) {
                    return;
                }
                eprintln!("ABUSE-PHASE {a}");
                let r = item_range.clone();
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.abuse_action(a, r))).is_err() {
                    eprintln!("ABUSE-CAUGHT {a}");
                }
            };
            const INSTANT: [&str; 10] = ["charge", "shot", "exit_shot", "fragment", "break", "shock", "emp", "flip", "copy_paste_undo", "contraption"];
            let tools: Vec<&str> = ["PinTool", "WoodPinTool", "BloodWireTool", "CableWireTool", "RopeTool", "ChainWireTool", "CopperWireTool", "EnergyWireTool", "HeatPipeTool", "RigidWireTool", "SpringWireTool", "StrongSpringWireTool", "DestructibleSpringWireTool", "BandageWireTool", "WoodStrutTool", "FixedCableTool", "SteelBindingTool", "WoodenBindingTool", "SliderTool", "UseWireTool", "RedUseWireTool", "BlueUseWireTool", "PushPowerTool", "PullPowerTool", "LiftPowerTool", "LightningTool", "PyroPowerTool", "PhaseLinkTool", "SerpentineBeltTool"].into_iter().filter(|t| !skipped(t)).collect();
            let tools_at = 400 + INSTANT.len() * 6;
            let drag_at = tools_at + tools.len() * 12;
            const LATE: [&str; 6] = ["ignite", "freeze", "slice", "explode", "delete_undo", "clear_map"];
            let late_at = drag_at + 60;
            if n < tools_at {
                if (n - 400) % 6 == 0 {
                    act(&mut g, INSTANT[(n - 400) / 6]);
                }
            } else if n < drag_at {
                let (k, w) = ((n - tools_at) / 12, (n - tools_at) % 12);
                let center = |g: &crate::game::Game, p: usize| {
                    let ph = g.physicals.get(p).filter(|ph| !ph.destroyed && g.body_alive(p))?;
                    let c = g.world.body(ph.rigidbody.body).sweep.c;
                    Some([c.x, c.y, 0.0])
                };
                if w == 0 {
                    eprintln!("ABUSE-PHASE {}", tools[k]);
                    let tool = tools[k];
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.set_tool_by_type_name(tool))).is_err() {
                        eprintln!("ABUSE-CAUGHT tool {tool}");
                    }
                }
                if (1..=6).contains(&w) {
                    if let Some(at) = if w == 1 { center(&g, item_physical) } else { center(&g, target_physical) } {
                        g.input.mouse = at;
                        g.input.screen = Some(crate::game::tool::Screen { mouse: [at[0] * 100.0, at[1] * 100.0], origin: [0.0; 2], scale: 0.01, view: None });
                        g.input.controls.insert("drag".into(), crate::game::tool::Control { down: w == 1, held: w < 6, up: w == 6 });
                    }
                }
            } else if n < late_at {
                // The drag tool grabs the item and swings it round the human for a second.
                let w = n - drag_at;
                if w == 0 {
                    eprintln!("ABUSE-PHASE drag");
                    g.set_tool_by_type_name("DragTool");
                }
                if let Some(ph) = g.physicals.get(item_physical).filter(|ph| !ph.destroyed && g.body_alive(item_physical)) {
                    let c = g.world.body(ph.rigidbody.body).sweep.c;
                    let t = w as f32 / 10.0;
                    g.input.mouse = if w < 3 { [c.x, c.y, 0.0] } else { [c.x + 3.0 * t.cos(), c.y + 3.0 * t.sin(), 0.0] };
                    g.input.controls.insert("drag".into(), crate::game::tool::Control { down: w == 1, held: (1..55).contains(&w), up: w == 55 });
                }
            } else if (n - late_at) % 40 == 0 && (n - late_at) / 40 < LATE.len() {
                act(&mut g, LATE[(n - late_at) / 40]);
            } else if n > late_at + LATE.len() * 40 + 20 {
                break;
            }
        }
        if let Err(m) = step(&mut g, 1.0 / 60.0) {
            return Err(format!("frame {n}: {m}"));
        }
    }
    for (i, l) in g.limbs.iter().enumerate() {
        let p = &g.physicals[l.PhysicalBehaviour];
        let c = &g.circulations[i];
        println!("{:14} {:?} active={} health={} bleeding={} wounds={}", p.name, &p.transform.position[..2], p.active, l.Health, c.BleedingRate, c.GunshotWoundCount);
    }
    Ok(())
}

pub fn run(start: &Start, rig: HumanRig, cache: PathBuf) -> Result<(), String> {
    PRELOAD.store(true, std::sync::atomic::Ordering::Relaxed);
    let dir = match start {
        Start::Map(dir, ..) | Start::Menu(dir) => Some(dir.clone()),
        Start::Recording(_) => None,
    };
    let (settings, controls, files) = crate::ui::load_settings(dir.as_deref());
    if let Some(game) = &dir {
        crate::achievements::init(&files.dir, Some(game));
        crate::game::stats::init(&files.dir);
        crate::ui::data::persistence_init(&files.dir);
    }
    let game = load(start, &rig, Some(settings.prefs()))?;
    let (map, item) = match start {
        Start::Map(_, map, item, _) => (Some(map.clone()), item.clone()),
        _ => (None, None),
    };
    let mode = if game.is_some() { crate::ui::Mode::Game } else { crate::ui::Mode::Menu };
    // PPG_SHOT=<png>: no visible window; saves the view after some frames and exits (the rendering check).
    let shot = std::env::var("PPG_SHOT").ok();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window { title: "People Playground rewrite".into(), resolution: { let (w, h) = shot_size(); let r = bevy::window::WindowResolution::new(w, h); if shot.is_some() { r.with_scale_factor_override(1.0) } else { r } }, visible: shot.is_none(), ..default() }),
                ..default()
            })
            .set(AssetPlugin { file_path: cache.to_string_lossy().into_owned(), ..default() })
            .set(ImagePlugin::default_nearest()),
    )
    .insert_resource(ClearColor(Color::srgb(0.055, 0.065, 0.08)))
    .insert_resource(rig)
    .insert_resource(crate::ui::new_ui(dir, mode, map, item))
    .insert_resource(settings)
    .insert_resource(controls)
    .insert_resource(files);
    if let Some(mut game) = game {
        // PPG_DETAIL: start in detail view (the rendering check of the limb health bars).
        game.ShowLimbStatus = std::env::var("PPG_DETAIL").is_ok();
        app.insert_resource(Play::new(game));
    }
    if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
        use bevy::render::{render_phase::sort_phase_system, Render, RenderSystems};
        render.add_systems(Render, refresh_mesh_depths.in_set(RenderSystems::PhaseSort).before(sort_phase_system::<bevy::core_pipeline::core_2d::Transparent2d>));
    }
    crate::lights::add(&mut app);
    app.add_plugins(crate::grab_pass::GrabPassPlugin);
    crate::limb_material::add(&mut app);
    crate::custom_material::add(&mut app);
    crate::tmp::material::add(&mut app);
    crate::mishap_fold_view::add(&mut app);
    crate::wires::add(&mut app);
    crate::delete_wire::add(&mut app);
    crate::ui::add(&mut app);
    // Sound for the live game; the rendering check (PPG_SHOT) and a recording's replay stay silent.
    if shot.is_none() && !matches!(start, Start::Recording(_)) {
        crate::audio_out::add(&mut app);
    }
    // PP_OPEN=<button identity> (with PPG_SHOT and --item): the item spawns at the map's center after 2 s, and 1.5 s
    // before the shot the first object with that button has it clicked, its dialog left open (the dialog screenshot check).
    if let (Ok(id), Some(_)) = (std::env::var("PP_OPEN"), &shot) {
        let at: f32 = std::env::var("PPG_AT").map_or(7.0, |v| v.parse().unwrap());
        app.add_systems(Update, move |time: Res<Time>, play: Option<ResMut<Play>>, mut step: Local<u8>| {
            let Some(mut play) = play else { return };
            let g = &mut play.game;
            let t = time.elapsed_secs();
            if std::env::var("PP_FXSTATE").is_ok() && (t * 2.0) as i32 != ((t - time.delta_secs()) * 2.0) as i32 {
                for p in g.physicals.iter().filter(|p| p.name.starts_with(&std::env::var("PP_FXSTATE").unwrap())) {
                    eprintln!("fxstate {t:.1} {} fire {} int {} prog {} temp {}", p.name, p.OnFire, p.burnIntensity, p.BurnProgress, p.Temperature);
                }
            }
            if *step == 0 && t > 2.0 {
                *step = 1;
                let item = g.catalog_selected.clone().expect("PP_OPEN needs --item");
                let mut db = g.assets.take().expect("the game's data");
                let c = g.BoundingBox.center;
                let spawned = g.catalog_spawn(&mut db, &item, [c[0], c[1], 0.0], false);
                g.assets = Some(db);
                g.unsynced.push(spawned.expect("PP_OPEN: the spawn").0);
            } else if *step == 1 && t > at - 1.5 && id == "Select" {
                // The starting item selected instead (the one spawned here is off screen): the selectable object
                // nearest the view's centre.
                *step = 2;
                let far = |q: usize| Vec2::from_slice(&g.physicals[q].transform.position[..2]).length();
                let p = (0..g.physicals.len()).filter(|&q| !g.physicals[q].destroyed && g.physicals[q].Selectable).min_by(|&a, &b| far(a).total_cmp(&far(b))).expect("PP_OPEN=Select: the item");
                g.select_for_test(p, false);
            } else if *step == 1 && t > at - 1.5 && id == "TelevisionUse" {
                // Every television used (TelevisionBehaviour.Use), as a click on it would.
                *step = 2;
                for tv in 0..g.televisions.len() {
                    g.television_use(tv);
                }
            } else if *step == 1 && t > at - 1.5 && id == "EnergySwordUse" {
                // Every energy sword switched on (EnergySwordBehaviour.Use) and spun at 720 degrees a second (its trail).
                *step = 2;
                for i in 0..g.energy_swords.len() {
                    g.energy_sword_use(i);
                    let p = g.energy_swords[i].physical;
                    let rb = g.physicals[p].rigidbody.clone();
                    rb.set_angular_velocity(&mut g.world, 720.0);
                }
            } else if *step == 1 && t > at - 1.5 && id == "ImmobilityFieldUse" {
                // Every immobility field switched on (ImmobilityFieldBehaviour.Use): its capture circle's GrabPass.
                *step = 2;
                for i in 0..g.immobility_fields.len() {
                    g.immobility_field_use(i);
                }
            } else if *step == 1 && t > at - 0.03 && id == "Activate" {
                // Every object activated once (the tool's direct activation): an effect's particles for the shot.
                *step = 2;
                for p in 0..g.physicals.len() {
                    if !g.physicals[p].destroyed {
                        g.activate_for_test(p);
                    }
                }
            } else if *step == 1 && t > at - 1.0 && id == "Colourpicker" {
                // The colour picker open on a hue of 0.6 (HSV 0.6, 0.7, 0.8).
                *step = 2;
                let mut b = crate::game::menu::ColourBox { hsv: [0.6, 0.7, 0.8], description: crate::il!(str "CathodeLightBehaviour::<Start>b__13_1" 1), hex: String::new(), editing: false, targets: Vec::new() };
                b.update_hex();
                g.colour = Some(b);
            } else if *step == 1 && t > at - 1.5 && id.starts_with("Ambient:") {
                // PP_OPEN=Ambient:<celsius>: the Environment panel's ambient temperature set (MapConfig.ApplySettings).
                *step = 2;
                let mut s = g.map_settings.expect("PP_OPEN=Ambient: a map");
                s.Ambient_temperature = id["Ambient:".len()..].parse().expect("PP_OPEN=Ambient:<celsius>");
                g.map_config_apply_settings(s);
            } else if *step == 1 && (id == "Fog" || id == "Rain" || id == "Snow") {
                // PP_OPEN=Fog|Rain|Snow: the Environment panel's toggle on at once (MapConfig.ApplySettings); the overlay fades in.
                *step = 2;
                let mut s = g.map_settings.expect("PP_OPEN=Fog: a map");
                match id.as_str() {
                    "Fog" => s.Fog = true,
                    "Rain" => s.Rain = true,
                    _ => s.Snow = true,
                }
                g.map_config_apply_settings(s);
            } else if *step == 1 && id == "Meltdown" {
                // PP_OPEN=Meltdown: each reactor core past MeltdownRequiredTime at once (ReactorBehaviour.Update's meltdown
                // branch: OnMeltdownStart, DisableOnMeltdown, the MeltdownSequence DoAtTime enabled).
                *step = 2;
                for r in 0..g.reactor_cores.len() {
                    let Some(q) = g.reactor_cores[r].sequence else { continue };
                    g.reactor_meltdown_start(r);
                    g.reactor_melt_down(r, q);
                    println!("meltdown {r}: events at {:?}", g.do_at_times[q].Events.iter().map(|e| e.Time).collect::<Vec<_>>());
                }
            } else if *step == 1 && t > at - 1.0 && id == "TyrePop" {
                // Every tyre popped (PopTyre with no point), a second to sag on the floor before the shot.
                *step = 2;
                for k in 0..g.tyres.len() {
                    g.tyre_pop(k, None);
                }
            } else if *step == 1 && t > at - 0.05 && id == "ArcFlash" {
                // Every ReactorA5ArcBehaviour flashed (its Flash event), its line at the start of the fade for the shot.
                *step = 2;
                for a in 0..g.reactor_a5_arcs.len() {
                    let id = g.reactor_a5_arcs[a].id;
                    g.reactor_a5_arc_flash(id);
                    let l = &g.lines[g.reactor_a5_arcs[a].line];
                    println!("arc {a}: {:?} .. {:?}", l.positions.first(), l.positions.last());
                }
            } else if *step == 1 && t > at - 1.5 && id == "ThermalVision" {
                // Thermal vision switched on (ThermalVisionBehaviour.ToggleThermalVision, as its key does).
                *step = 2;
                let on = g.thermal_vision.expect("the scene's ThermalVisionBehaviour");
                g.thermal_vision_toggle(on);
            } else if *step == 1 && t > at - 1.5 && id == "ClearEverything" {
                // The Clear everything button pressed instead.
                *step = 2;
                g.clear_everything();
            } else if *step == 1 && t > at - std::env::var("PP_IGNITE_BEFORE").map_or(4.0, |v| v.parse().unwrap()) && id == "Ignite" {
                // PP_OPEN=Ignite: every object set alight (Ignite(true)), PP_IGNITE_BEFORE (4) seconds before the shot.
                *step = 2;
                eprintln!("FX bbox {:?} {:?}", g.BoundingBox.center, g.BoundingBox.extents);
                for p in 0..g.physicals.len() {
                    if !g.physicals[p].destroyed {
                        // PP_HEAT: kept alight at 1000 degrees (the recorder's igniteall heats its item so).
                        if std::env::var("PP_HEAT").is_ok() && !g.physicals[p].isStatic {
                            g.physicals[p].Temperature = 1000.0;
                        }
                        g.ignite(p, true);
                    }
                }
            } else if *step == 1 && t > at - 1.0 && id == "UseHold" {
                // PP_OPEN=UseHold: every object used continuously (the held activate key) for the last second.
                *step = 2;
                for p in 0..g.physicals.len() {
                    if !g.physicals[p].destroyed {
                        g.continuous.listeners.push(crate::game::continuous::Listener::Hold(p));
                    }
                }
                g.continuous.hold_left = u32::MAX;
            } else if *step == 1 && t > at - std::env::var("PP_EXPLODE_BEFORE").map_or(0.15, |v| v.parse().unwrap()) && id == "Explode" {
                // PP_OPEN=Explode: a medium explosion on the last object spawned, PP_EXPLODE_BEFORE (0.15) seconds before the shot.
                *step = 2;
                let c = g.physicals.last().expect("PP_OPEN=Explode: the item").transform.position;
                eprintln!("FX explode at {c:?}");
                g.create_explosion_with_water(false, crate::game::explosion::ExplosionParameters::new(32, [c[0], c[1], 0.0], 6.0, 12.0, true, crate::game::explosion::EffectSize::Medium, 0.5, 0));
            } else if *step == 1 && t > at - 1.5 && !matches!(id.as_str(), "Ignite" | "UseHold" | "Explode") && id != "Activate" && id != "ArcFlash" && id != "TyrePop" && id != "Colourpicker" && id != "Fog" && id != "Rain" && id != "Snow" && id != "Meltdown" && !id.starts_with("Ambient:") {
                *step = 2;
                let found = (0..g.physicals.len()).find_map(|q| g.physicals[q].menu.iter().find(|b| b.Identity == id).map(|b| b.actions.clone()));
                g.context_menu_click(&found.unwrap_or_else(|| panic!("PP_OPEN: no {id} button")));
            }
        });
    }
    // PP_SPAWN_BENCH=<item,item,..|list>: spawns each item at frame 150+90k, then the whole list again; logs the spawn call and the
    // wall ms of the next frames (stderr "BENCH ...").
    if let Ok(list) = std::env::var("PP_SPAWN_BENCH") {
        app.add_systems(Update, move |time: Res<Time<Real>>, play: Option<ResMut<Play>>, mut frame: Local<u32>, mut since: Local<(String, u32)>, mut exit: MessageWriter<AppExit>| {
            let Some(mut play) = play else { return };
            let g = &mut play.game;
            *frame += 1;
            let ms = time.delta_secs() * 1000.0;
            if since.1 > 0 {
                eprintln!("BENCH frame {} {} +{} {ms:.2} ms", *frame, since.0, 5 - since.1);
                since.1 -= 1;
            } else if *frame % 60 == 0 {
                eprintln!("BENCH idle {ms:.2} ms");
            }
            let mut db = g.assets.take().expect("the game's data");
            if list == "list" {
                eprintln!("BENCH names {:?}", Game::catalog_names(&mut db));
                exit.write(AppExit::Success);
                g.assets = Some(db);
                return;
            }
            let items: Vec<&str> = list.split(',').collect();
            let k = (*frame as i64 - 150) / 90;
            if *frame >= 150 && (*frame - 150) % 90 == 0 {
                if k as usize >= 2 * items.len() {
                    exit.write(AppExit::Success);
                } else {
                    let name = items[k as usize % items.len()];
                    let round = if (k as usize) < items.len() { "first" } else { "repeat" };
                    let c = g.BoundingBox.center;
                    let t = std::time::Instant::now();
                    let spawned = g.catalog_spawn(&mut db, name, [c[0] + (k % items.len() as i64) as f32 * 2.0 - 4.0, c[1] + 1.0, 0.0], false);
                    eprintln!("BENCH spawn {name} {round} call {:.2} ms", t.elapsed().as_secs_f64() * 1000.0);
                    g.unsynced.push(spawned.expect("spawn").0);
                    *since = (format!("{name}/{round}"), 5);
                }
            }
            g.assets = Some(db);
        });
    }
    // PP_WALLSHOT=<gap>,<dx>,<dy> (with PPG_SHOT, the wall check; tools/recorder PPG_WALL + PPG_SHOTGUN): at 1 s a Human
    // <gap> m right of the bounding box's left edge, at 7 s a flipped Shotgun <dx>,<dy> from its UpperBody, fired 0.5 s later.
    if let (Ok(v), Some(_)) = (std::env::var("PP_WALLSHOT"), &shot) {
        let v: Vec<f32> = v.split(',').map(|x| x.parse().unwrap()).collect();
        // PP_WALLSHOT_AFTER=<s>: the screenshot <s> game seconds after the shot (the game clock, not the frame clock).
        let after: Option<f32> = std::env::var("PP_WALLSHOT_AFTER").ok().map(|v| v.parse().unwrap());
        let path = shot.clone().unwrap();
        app.add_systems(Update, move |time: Res<Time>, play: Option<ResMut<Play>>, mut step: Local<(u8, usize)>, mut fired: Local<f32>, mut commands: Commands, mut exit: MessageWriter<AppExit>| {
            let Some(mut play) = play else { return };
            let g = &mut play.game;
            let t = time.elapsed_secs();
            if let (Some(a), 3) = (after, step.0) {
                if g.time.time >= *fired + a {
                    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
                    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
                    eprintln!("WALLSHOT shot at game {} fired {}", g.time.time, *fired); *step = (4, 0);
                }
                return;
            }
            if step.0 == 4 {
                if g.time.time >= *fired + after.unwrap() + 0.3 { exit.write(AppExit::Success); }
                return;
            }
            let mut db = match (step.0, t) { (0, 1.0..) | (1, 7.0..) => g.assets.take().expect("the game's data"), (2, 7.5..) => { g.activate_for_test(step.1); *fired = g.time.time; *step = (3, 0); return } _ => return };
            let b = g.BoundingBox.clone(); if step.0 == 0 { for (i, p) in g.physicals.iter().enumerate().filter(|(i, p)| !p.destroyed && g.body_alive(*i)) { let c = g.world.body(p.rigidbody.body).sweep.c; eprintln!("WALLSHOT map {i} {} {:.3} {:.3} sprite {:?}", p.name, c.x, c.y, p.transform.localScale); } eprintln!("WALLSHOT bbox {:?} {:?}", b.center, b.extents); }
            let at = if step.0 == 0 { [b.center[0] - b.extents[0] + v[0], 0.0, 0.0] } else {
                let ub = g.physicals.iter().position(|p| p.name == "UpperBody").expect("PP_WALLSHOT: no UpperBody");
                let c = g.world.body(g.physicals[ub].rigidbody.body).sweep.c;
                [c.x + v[1], c.y + v[2], 0.0]
            };
            let first = g.physicals.len();
            let spawned = g.catalog_spawn(&mut db, if step.0 == 0 { "Human" } else { "Shotgun" }, at, step.0 == 1);
            g.assets = Some(db);
            g.unsynced.push(spawned.expect("PP_WALLSHOT: the spawn").0);
            eprintln!("WALLSHOT step {} at {:?}", step.0, at);
            *step = (step.0 + 1, first);
        });
    }
    // PP_PORTAL=<scene> (with PPG_SHOT): the portal gun's check (extras_view.rs).
    if let (Ok(scene), Some(_)) = (std::env::var("PP_PORTAL"), &shot) {
        crate::extras_view::add_test(&mut app, scene);
    }
    // PP_MISHAP=<scene> (with PPG_SHOT): the portal mishaps' check (mishap_view.rs).
    if let (Ok(scene), Some(_)) = (std::env::var("PP_MISHAP"), &shot) {
        crate::mishap_view::add_test(&mut app, scene);
    }
    // PP_FX3=belt|hidden|preview|lightning (with PPG_SHOT; ISSUES.md #21/#24): tools/recorder/Fx3VisProbe.cs's scene.
    if let (Ok(scene), Some(path)) = (std::env::var("PP_FX3"), &shot) {
        fx3_scene(&mut app, scene, path.clone());
    }
    if let Some(path) = shot {
        // The map's lights come on over the first seconds (MapLightBehaviour.DelayedActivation).
        app.add_systems(Update, move |time: Res<Time>, mut taken: Local<bool>, mut commands: Commands, mut exit: MessageWriter<AppExit>| {
            let t = time.elapsed_secs();
            if t > std::env::var("PPG_AT").map_or(7.0, |v| v.parse().unwrap()) && !*taken {
                *taken = true;
                use bevy::render::view::screenshot::{save_to_disk, Screenshot};
                commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.clone()));
            } else if *taken && std::path::Path::new(&path).exists() {
                // Saved: a slow GPU reads the window back after more than the half second this once waited.
                exit.write(AppExit::Success);
            }
        });
    }
    app.run();
    Ok(())
}

/// PP_FX3: Fx3VisProbe.cs's scenes (after the map lights). At 4 s two Wheels at (1, -1) and (5, -1) with a belt from the first; "hidden" adds two
/// at (1, 3) and (5, 3) with an Invisible belt at 4.5 s and makes the tools invisible at 5 s; "preview" chooses the
/// SerpentineBeltTool at 5 s and gives its line (-6, -2) to (-1, 1), enabled; "energy" chooses the EnergyWireTool at 5 s
/// and lays its wire from the first wheel to the second; "glow" moves the lightning's LightSprite to (-4, 4) and shows it
/// (no bolt, explosion or shake); "lightning" moves the map's
/// WeatherLightningBehaviour to (-4, 4) with Width 0 at 5 s and starts FireBolt, and shoots the first frame its
/// LightSprite shows (PPG_SHOT with -flash before .png). The shot is PPG_SHOT at PPG_AT.
fn fx3_scene(app: &mut App, scene: String, path: String) {
    app.add_systems(Update, move |mut commands: Commands, time: Res<Time>, play: Option<ResMut<Play>>, mut step: Local<u32>, mut wheels: Local<Vec<usize>>| {
        let Some(mut play) = play else { return };
        let g = &mut play.game;
        let t = time.elapsed_secs();
        let mut spawn = |g: &mut crate::game::Game, at: [f32; 2]| {
            let mut db = g.assets.take().expect("the game's data");
            let first = g.physicals.len();
            let (inst, _) = g.catalog_spawn(&mut db, "Wheel", [at[0], at[1], 0.0], false).expect("PP_FX3: the spawn");
            g.assets = Some(db);
            g.unsynced.push(inst);
            first
        };
        match *step {
            0 if t >= 4.0 => {
                *step = 1;
                wheels.push(spawn(g, [1.0, -1.0]));
                wheels.push(spawn(g, [5.0, -1.0]));
                if scene == "hidden" {
                    wheels.push(spawn(g, [1.0, 3.0]));
                    wheels.push(spawn(g, [5.0, 3.0]));
                }
            }
            1 if t >= 4.1 => {
                *step = 2;
                g.belt_add(wheels[0], wheels[1], false);
            }
            2 if t >= 4.5 => {
                *step = 3;
                if scene == "hidden" {
                    g.belt_add(wheels[2], wheels[3], true);
                }
            }
            3 if t >= 5.0 => {
                *step = 4;
                match scene.as_str() {
                    "hidden" => g.tool_wheel_set_tool_visibility(true),
                    "preview" => {
                        g.set_tool_by_type_name("SerpentineBeltTool");
                        let l = g.belt_tool.line.expect("the belt tool's line");
                        g.lines[l].positions = vec![[-6.0, -2.0, 0.0], [-1.0, 1.0, 0.0]];
                        g.lines[l].enabled = true;
                    }
                    "energy" => {
                        g.set_tool_by_type_name("EnergyWireTool");
                        g.wire_lay_between(wheels[0], wheels[1]);
                    }
                    "glow" => {
                        let l = g.map_lightning.expect("the map's lightning");
                        g.weather_lightning_light_to(l, [-4.0, 4.0, 0.0]);
                        let r = g.lightnings[l].light.expect("the lightning's LightSprite");
                        g.set_sprite_enabled(crate::game::visual::SpriteAt::Renderer(r), true);
                    }
                    "lightning" => {
                        let l = g.map_lightning.expect("the map's lightning");
                        g.lightnings[l].position = [-4.0, 4.0, 0.0];
                        g.lightnings[l].Width = 0.0;
                        g.weather_lightning_fire(l);
                    }
                    _ => {}
                }
            }
            4 if scene == "lightning" => {
                let l = g.map_lightning.unwrap();
                if let Some(r) = g.lightnings[l].light.filter(|&r| g.renderers[r].enabled) {
                    *step = 5;
                    // The frame the flash shows (the probe's shot); the clock runs on, the flash lasts 0.032 s.
                    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
                    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.replace(".png", "-flash.png")));
                    let x = &g.renderers[r];
                    eprintln!("FX3 light at {t} chain {:?} line {:?}", x.chain.iter().map(|n| n.t).collect::<Vec<_>>(), g.lightnings[l].line.map(|i| g.lines[i].enabled));
                }
            }
            _ => {}
        }
    });
}

/// A frame panic (an unrewritten feature) comes back as its message.
pub(crate) fn step(g: &mut Game, real: f32) -> Result<(), String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let t = &mut g.time;
        // Time.deltaTime: the unscaled frame time clamped to maximumDeltaTime, then scaled.
        let dt = real.min(t.maximumDeltaTime) * t.timeScale;
        // TimeManager::Update's smoothDeltaTime: the counter c = c * 0.8 + 0.2 weights the frame by 0.2 / c.
        t.smoothCount = t.smoothCount * 0.8 + 0.2;
        let w = 0.2 / t.smoothCount;
        t.smoothDeltaTime = (1.0 - w) * t.smoothDeltaTime + w * dt;
        let frame_time = t.timeAsDouble + dt as f64;
        // TimeManager::Update (the first system of the loop): the frame count, the dynamic time and the clock.
        t.frameCount += 1;
        t.dynamicTimeAsDouble = frame_time;
        t.realtimeSinceStartup += real as f64;
        // TimeManager::Update (1805353e0): the unscaled time is the real clock, and its delta (+0x7c) is not clamped.
        let unscaled = t.unscaledTimeAsDouble + real as f64;
        let unscaled_dt = (unscaled - t.unscaledTimeAsDouble) as f32;
        t.unscaledTimeAsDouble = unscaled;
        // EarlyUpdate's RendererScene::NotifyInvisible.
        crate::particles::notify_invisible(g, frame_time);
        while g.time.fixedTimeAsDouble + g.time.fixedDeltaTime as f64 <= frame_time {
            let t = &mut g.time;
            t.fixedTimeAsDouble += t.fixedDeltaTime as f64;
            t.fixedTime = t.fixedTimeAsDouble as f32;
            (t.timeAsDouble, t.time, t.deltaTime) = (t.fixedTimeAsDouble, t.fixedTime, t.fixedDeltaTime);
            // StepFixedTime (180535260): with a non-zero timeScale the fixed step's unscaled time is the frame's
            // unscaled time less the fixed step's lag behind the frame, unscaled; at timeScale 0 it keeps the last.
            if t.timeScale != 0.0 {
                let u = (t.fixedTimeAsDouble - frame_time) / t.timeScale as f64 + t.unscaledTimeAsDouble;
                t.fixedUnscaledDeltaTime = (u - t.fixedUnscaledTimeAsDouble) as f32;
                t.fixedUnscaledTimeAsDouble = u;
            }
            t.unscaledDeltaTime = t.fixedUnscaledDeltaTime;
            g.fixed_update_scripts();
            g.simulate_physics();
        }
        let t = &mut g.time;
        (t.timeAsDouble, t.time, t.deltaTime, t.unscaledDeltaTime) = (frame_time, frame_time as f32, dt, unscaled_dt);
        g.interpolate_bodies();
        g.update_scripts();
        g.extras_update();
        g.mishap_update();
        g.decal_streaks_update();
        g.limb_view_update();
        // OnWillRenderObject for the limbs the camera draws: renderer.bounds against the orthographic view (the
        // sprites sit between its near and far planes, so the frustum test is the rectangle overlap).
        g.renderer_cull_begin();
        for l in 0..g.limbs.len() {
            if !g.limb_active(l) {
                continue;
            }
            let drawn = g.input.view.is_none_or(|(min, max)| {
                let (c, e) = g.sprite_world_bounds(g.limbs[l].PhysicalBehaviour);
                (0..2).all(|i| c[i] + e[i] >= min[i] && c[i] - e[i] <= max[i])
            });
            if drawn {
                g.skin_will_render(l);
            }
        }
        g.syringes_will_render();
        g.tyres_will_render();
        g.waters_will_render();
        g.foliage_will_render(g.input.view);
        g.displays_will_render();
        g.visual_will_render();
        g.heating_elements_will_render();
        g.gates_will_render();
        g.liquid_level_gates_will_render();
        g.generators_will_render();
        g.toggleable_mirrors_will_render();
        g.valves_will_render();
        g.dangles_will_render();
        g.detectors_will_render();
        g.floodlights_will_render();
        g.glowing_metals_will_render();
        g.burnable_sprites_will_render();
        g.miniguns_will_render();
        g.whs_will_render();
        g.beamformers_will_render();
        g.resizable_slices_will_render();
        g.key_triggers_will_render();
        g.centrifuges_will_render();
        g.liquid_containers_will_render();
        g.police_boxes_will_render();
        g.determined_anomalies_will_render();
        g.physicals_will_render();
        g.reactor_a5_displays_will_render();
        g.v45_will_render();
        g.renderer_cull_end();
        g.end_of_frame_calls();
    }))
    .map_err(panic_message)
}

/// A physical object's own sprite (the limbs have their own material).
#[derive(Component)]
#[require(MainScene)]
/// The physical, and the scale from the sprite's units to the world (sliced and tiled sprites are laid out in
/// pixels so their borders keep their texel size).
pub(crate) struct ObjectSprite(pub(crate) usize, f32);

/// A SpriteRenderer on an object without a PhysicalBehaviour (`Game.renderers`), and the same scale.
#[derive(Component)]
#[require(MainScene)]
pub(crate) struct RendererSprite(pub(crate) usize, f32);

/// A limb's LimbStatus SpriteRenderer (part 0 the frame with BarSprite, 1 with DeadSprite, 2 the bar), with its scale.
#[derive(Component)]
#[require(MainScene)]
pub(crate) struct LimbStatusSprite(usize, u8, f32);

/// Each decoded texture with its width and height.
/// Each Texture2D's Image, shared by every drawing system: `preload_gpu_textures` adds all the decoded ones at load, so none
/// uploads on first sight mid-game.
#[derive(Resource, Default, Deref, DerefMut)]
pub struct Textures(pub std::collections::HashMap<(usize, i64), (Handle<Image>, [f32; 2])>);

/// The Texture2Ds `Game::preload_catalog` decoded, made Images (uploaded with the next frame) as soon as the game's data is there.
pub fn preload_gpu_textures(play: Option<Res<Play>>, mut images: ResMut<Assets<Image>>, mut textures: ResMut<Textures>) {
    let Some(db) = play.as_ref().and_then(|p| p.game.assets.as_deref()) else { return };
    for (t, d) in db.take_decoded() {
        // An empty texture (or one whose pixels do not fill it) waits for its first use, as before.
        let whole = d.0.width > 0 && d.0.height > 0 && d.1.len() == d.0.width as usize * d.0.height as usize * 4;
        if whole && !textures.contains_key(&(t.file, t.path_id)) {
            upload_texture(&mut images, &mut textures, t, &d.0, &d.1);
        }
    }
}

/// TagManager.asset's m_SortingLayers uniqueIDs, in draw order (Bottom first). A SpriteRenderer's sortingLayerID is one of them.
pub(crate) const SORTING_LAYERS: [u32; 8] = [334215217, 1370879715, 0, 182652991, 1377110059, 2185794113, 2257819283, 2607693843];
/// Their names (TagManager.asset), in the same order.
pub(crate) const SORTING_LAYER_NAMES: [&str; 8] = ["Bottom", "Background", "Default", "Portals", "Foreground", "Bubbles", "Top", "Decals"];

/// A renderer's material's render queue; no material is the default sprite material (Transparent).
pub(crate) fn render_queue(db: &AssetDb, material: Option<crate::assets::db::ObjRef>) -> i16 {
    let Some(r) = material else { return 3000 };
    let m = crate::assets::components::Material::read(db.data(r)).unwrap();
    if m.render_queue != -1 {
        return m.render_queue as i16;
    }
    // By path id alone: the shader may sit in a built-in file we don't load.
    let shader = db.material_shader_id(r.file, m.shader);
    crate::shader_queues::SHADER_QUEUES.iter().find(|q| q.0 == shader).unwrap_or_else(|| unimplemented!("mod-only: material {r:?}: shader has no queue")).1
}

/// A sprite's draw key, in ForwardShaderRenderLoop::RenderObjectSorter<0>'s order (18061e340): its sorting layer's index
/// and sortingOrder (the layering key), its render queue, its distance (BuildRenderObjectData 18060d470, the
/// orthographic sort: view z of the bounds centre minus the renderer's sortingFudge, smaller first), then its group
/// (objects before renderers) and index (the spawn order). The sorter's later ties (sorting group, sameDistanceSortPriority,
/// material instance ID, subset, roIndex) stand as the group and index.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SortKey {
    layer: usize,
    order: i16,
    queue: i16,
    /// The distance's f32 as a total-order integer.
    distance: i32,
    group: u8,
    index: usize,
}

/// A wire's LineRenderer: sortingLayerName "Top" (TagManager's seventh), after the sprites of its order.
pub(crate) fn line_key(order: i16, queue: i16, index: usize) -> SortKey {
    SortKey { layer: 6, order, queue, distance: sort_distance(0.0, 0.0), group: 2, index }
}

/// A line on a sprite's sorting layer (`sortingLayerID`) and order.
pub(crate) fn sprite_line_key(sorting_layer: i32, order: i16, queue: i16, index: usize) -> SortKey {
    SortKey { layer: sorting_layer_index(sorting_layer), order, queue, distance: sort_distance(0.0, 0.0), group: 2, index }
}

/// The main camera's world z (Game.camera.z), set at the scene's load: the sort distances' camera.
pub(crate) static CAMERA_Z: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0xc120_0000);

/// BuildRenderObjectData's orthographic distance for a bounds centre at world `z`: the view z (the unrotated camera's
/// ((y * 0 + x * 0) + z * -1) + camera z) minus the sortingFudge, as a total-order integer (-0 ties 0, as the sorter's
/// float compare).
pub(crate) fn sort_distance(z: f32, fudge: f32) -> i32 {
    let camera = f32::from_bits(CAMERA_Z.load(std::sync::atomic::Ordering::Relaxed));
    let d = (-z + camera) - fudge;
    let d = if d == 0.0 { 0.0 } else { d };
    let bits = d.to_bits() as i32;
    if bits < 0 { bits ^ 0x7fff_ffff } else { bits }
}

/// A ParticleSystemRenderer copy's subset on its sorting layer and order, its world bounds centre at `z`
/// (CalculateWorldBounds) less the renderer's sortingFudge.
pub(crate) fn particle_key(sorting_layer: i32, order: i16, queue: i16, z: f32, fudge: f32, index: usize) -> SortKey {
    SortKey { layer: sorting_layer_index(sorting_layer), order, queue, distance: sort_distance(z, fudge), group: 2, index }
}

/// A sprite key for a renderer whose bounds centre is at world `z`.
pub(crate) fn sprite_key(sorting_layer: i32, order: i16, queue: i16, z: f32, group: u8, index: usize) -> SortKey {
    SortKey { layer: sorting_layer_index(sorting_layer), order, queue, distance: sort_distance(z, 0.0), group, index }
}

/// A sprite's world bounds centre z: its local bounds centre (`c`, z 0 for a sprite) through localToWorld. A flat renderer
/// only has m[14]; one tilted out of the xy plane (the shipped trees and bushes) takes the centre's x and y in.
fn bounds_z(m: &[f32; 16], c: [f32; 3]) -> f32 {
    if m[2] == 0.0 && m[6] == 0.0 {
        return m[14];
    }
    m[2] * c[0] + m[6] * c[1] + m[10] * c[2] + m[14]
}

/// EnergySwordMotionBlur's MeshRenderer (energy_blur.rs), after the lines of its order.
pub(crate) fn blur_key(sorting_layer: i32, order: i16, queue: i16, index: usize) -> SortKey {
    SortKey { layer: sorting_layer_index(sorting_layer), order, queue, distance: sort_distance(0.0, 0.0), group: 5, index }
}

/// A renderer's sortingLayerID and sortingOrder set at run time: the key changes (and object_sprites ranks again) only when
/// they differ.
pub(crate) fn resort(key: &mut Mut<SortKey>, sorting_layer: i32, order: i16) {
    let layer = sorting_layer_index(sorting_layer);
    if (key.layer, key.order) != (layer, order) {
        (key.layer, key.order) = (layer, order);
    }
}

/// The index of a sortingLayerID in TagManager's order.
pub(crate) fn sorting_layer_index(id: i32) -> usize {
    SORTING_LAYERS.iter().position(|&u| u == id as u32).unwrap_or_else(|| unimplemented!("mod-only: SpriteRenderer sortingLayerID {id} is not in TagManager's sortingLayers"))
}

/// Bevy freezes a Mesh2d's sort key when it is first queued, and every sprite's z moves as new ones are ranked.
fn refresh_mesh_depths(
    mut phases: ResMut<bevy::render::render_phase::ViewSortedRenderPhases<bevy::core_pipeline::core_2d::Transparent2d>>,
    meshes: Res<bevy::sprite_render::RenderMesh2dInstances>,
) {
    for phase in phases.values_mut() {
        for item in phase.items.values_mut() {
            if let Some(mesh) = meshes.get(&item.entity.1) {
                item.sort_key = bevy::math::FloatOrd(mesh.transforms.world_from_local.translation.z);
            }
        }
    }
}

/// Each key's z: its rank in draw order over all the keys, spread over -0.5 (drawn first) to 0.5 (drawn last).
fn sprite_depths(keys: &[SortKey]) -> Vec<f32> {
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by_key(|&i| keys[i]);
    let n = keys.len() as f32;
    let mut z = vec![0.0; keys.len()];
    for (rank, &i) in order.iter().enumerate() {
        z[i] = -0.5 + (rank as f32 + 0.5) / n;
    }
    z
}

/// One pooled SpriteRenderer child of SpawnableOutlineBehaviour: its index in the pool and the Size its mesh was made for.
#[derive(Component)]
#[require(MainScene)]
pub(crate) struct OutlineRect;

/// A SpriteRenderer read from the scene, made into a sprite of draw size `size` and inserted on `entity`.
pub(crate) fn insert_scene_sprite(g: &Game, rect: &RectPrefab, size: [f32; 2], images: &mut Assets<Image>, meshes: &mut Assets<Mesh>, materials: &mut Assets<ColorMaterial>, glows: &mut Assets<crate::lights::Glow>, panels: &mut Assets<crate::lights::Panels>, parts: &mut Assets<crate::particles::ParticleMaterial>, textures: &mut Textures, entity: EntityCommands) -> bool {
    let Some(db) = g.assets.as_deref() else { return false };
    let draw = ([false, false], rect.draw_mode, size, rect.threshold, rect.adaptive);
    let Some((drawn, _)) = make_sprite(&g.limb_sprite_cache, db, images, meshes, materials, glows, panels, parts, rect.material, textures, rect.sprite, rect.color, draw) else { return false };
    drawn.insert(entity, rect.color);
    true
}

/// What SpawnableOutlineBehaviour.RectPrefab's SpriteRenderer holds (read from the install's scene).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RectPrefab {
    sprite: crate::assets::db::ObjRef,
    material: Option<crate::assets::db::ObjRef>,
    color: [f32; 4],
    draw_mode: i32,
    threshold: f32,
    adaptive: bool,
    game_layer: i32,
    sorting_layer: i32,
    order: i16,
}

fn read_rect_prefab(db: &mut crate::assets::db::AssetDb) -> Option<RectPrefab> {
    let s = *db.find_scripts("SpawnableOutlineBehaviour").ok()?.first()?;
    let (_, _, v) = db.mono(s).ok()??;
    let root = db.resolve(s.file, v.get("RectPrefab").and_then(crate::assets::mono::Value::pptr)?)?;
    scene_sprite(db, root)
}

/// The SpriteRenderer on GameObject `root` as the scene stores it.
pub(crate) fn scene_sprite(db: &mut crate::assets::db::AssetDb, root: crate::assets::db::ObjRef) -> Option<RectPrefab> {
    let prefab = db.prefab(root).ok()?;
    let object = &prefab.objects[0];
    object.components.iter().find_map(|(_, c)| match c {
        crate::assets::db::Component::SpriteRenderer(r) => Some(RectPrefab {
            sprite: db.resolve(root.file, r.sprite)?,
            material: r.materials.first().and_then(|m| db.resolve(root.file, *m)),
            color: r.color,
            draw_mode: r.draw_mode,
            threshold: r.adaptive_mode_threshold,
            adaptive: r.sprite_tile_mode == 1,
            game_layer: object.layer as i32,
            sorting_layer: r.sorting_layer_id,
            order: r.sorting_order,
        }),
        _ => None,
    })
}

/// SpawnableOutlineBehaviour's pooled children drawn as RectPrefab's SpriteRenderer (Sliced `outline sprite`, its material and colour):
/// each active child at the mouse transform's position, localScale.x `flip`, with its localPosition, z angle and `size`.
pub(crate) fn spawnable_outline_sprites(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut glows: ResMut<Assets<crate::lights::Glow>>,
    mut panels: ResMut<Assets<crate::lights::Panels>>,
    mut parts: ResMut<Assets<crate::particles::ParticleMaterial>>,
    mut play: ResMut<Play>,
    mut textures: ResMut<Textures>,
    mut prefab: Local<Option<Option<RectPrefab>>>,
    mut pool: Local<Vec<(Entity, [f32; 2])>>,
    mut placed: Query<(Entity, &SortKey, &mut Transform)>,
) {
    // A new Main scene: the old pool went with the old one.
    if play.is_added() {
        pool.clear();
    }
    let g = &mut play.game;
    let Some(db) = g.assets.as_deref_mut() else { return };
    let Some(rect) = prefab.get_or_insert_with(|| read_rect_prefab(db)).as_ref() else { return };
    let o = g.spawnable_outline.clone();
    let db = g.assets.as_deref().unwrap();
    for (i, c) in o.children.iter().enumerate() {
        let visible = o.active && c.active;
        let draw = ([false, false], rect.draw_mode, c.size, rect.threshold, rect.adaptive);
        let sign = if o.flip < 0.0 { -1.0 } else { 1.0 };
        let at = Vec3::new(o.position[0] + c.position[0] * o.flip, o.position[1] + c.position[1], 0.0);
        let rotation = Quat::from_rotation_z((c.angle * sign).to_radians());
        let visibility = if visible { Visibility::Inherited } else { Visibility::Hidden };
        if i >= pool.len() {
            let Some((drawn, _)) = make_sprite(&g.limb_sprite_cache, db, &mut images, &mut meshes, &mut materials, &mut glows, &mut panels, &mut parts, rect.material, &mut textures, rect.sprite, rect.color, draw) else { return };
            let key = SortKey { layer: sorting_layer_index(rect.sorting_layer), order: rect.order, queue: render_queue(db, rect.material), distance: sort_distance(0.0, 0.0), group: 4, index: i };
            // Ranked in draw order among every sprite already placed.
            let mut keys: Vec<SortKey> = placed.iter().map(|(_, k, _)| *k).collect();
            keys.push(key);
            let depths = sprite_depths(&keys);
            for ((_, _, mut t), &z) in placed.iter_mut().zip(&depths) {
                t.translation.z = z;
            }
            let mut entity = commands.spawn((OutlineRect, Transform { translation: Vec3::new(at.x, at.y, *depths.last().unwrap()), rotation, ..default() }, key, visibility, crate::lights::camera_layers(rect.game_layer)));
            let id = entity.id();
            drawn.insert(entity.reborrow(), rect.color);
            pool.push((id, c.size));
            continue;
        }
        let (e, size) = pool[i];
        if size != c.size {
            let Some((drawn, _)) = make_sprite(&g.limb_sprite_cache, db, &mut images, &mut meshes, &mut materials, &mut glows, &mut panels, &mut parts, rect.material, &mut textures, rect.sprite, rect.color, draw) else { return };
            drawn.insert(commands.entity(e), rect.color);
            pool[i].1 = c.size;
        }
        if let Ok((_, _, mut t)) = placed.get_mut(e) {
            t.translation.x = at.x;
            t.translation.y = at.y;
            t.rotation = rotation;
        }
        commands.entity(e).insert(visibility);
    }
    // Children past the pool's count do not exist; the pool keeps its entities (SetOutline deactivates the surplus).
}

/// sharedassets0's Unlit/AdditiveGlowSprite (1.28.41's path ids, `AssetDb::shader_id`).
const ADDITIVE_GLOW_SHADER: i64 = 891;
/// sharedassets0's Unlit/Jaap/PanelsShader.
const PANELS_SHADER: i64 = 888;
/// The built-in Particles/Standard Unlit (render_queue's path id).
const PARTICLES_SHADER: i64 = 34;

/// A Particles/Standard Unlit sprite's vertex colour: the SpriteRenderer colour, linear in the game's linear colour space.
fn vertex_colour(mesh: &mut Mesh, c: [f32; 4]) {
    let linear = Color::srgba(c[0], c[1], c[2], c[3]).to_linear().to_f32_array();
    let n = mesh.count_vertices();
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![linear; n]);
}

/// A sprite made this frame, with the key that ranks it.
enum NewSprite {
    Object(usize, f32, Drawn, i32),
    Renderer(usize, f32, Drawn, i32),
    Status(usize, u8, f32, Drawn),
}

/// A sprite from the game's data for each non-limb physical object and each other SpriteRenderer without one;
/// textures decoded once. Every sprite's z is its rank in draw order (sorting layer, sortingOrder, spawn order) over all
/// the sprites, so a new one is ranked among the placed ones and they are re-ranked with it.
pub(crate) fn object_sprites(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut glows: ResMut<Assets<crate::lights::Glow>>,
    mut panels: ResMut<Assets<crate::lights::Panels>>,
    mut parts: ResMut<Assets<crate::particles::ParticleMaterial>>,
    mut play: ResMut<Play>,
    mut textures: ResMut<Textures>,
    mut statuses: Local<usize>,
    mut placed: Query<(Entity, &SortKey, &mut Transform)>,
    limbs_added: Query<(), (Added<SortKey>, Or<(With<LimbSprite>, With<crate::wires::WireLine>, With<crate::decals::DecalSprite>, With<crate::particles::TrailEntity>)>)>,
    // resprite changed a sprite's render queue.
    resorted: Query<(), (Changed<SortKey>, Without<LimbSprite>)>,
) {
    if play.is_added() {
        *statuses = 0;
    }
    let mut done = (play.done.1, play.done.2);
    let g = &play.game;
    let Some(db) = g.assets.as_deref() else { return };
    let limbs: Vec<usize> = g.limbs.iter().map(|l| l.PhysicalBehaviour).collect();
    let mut new: Vec<(SortKey, NewSprite)> = Vec::new();
    for p in done.0..g.physicals.len() {
        let Some((r, color, order)) = g.physicals[p].sprite else { continue };
        if limbs.contains(&p) {
            continue;
        }
        let Some((drawn, scale)) = make_sprite(&g.limb_sprite_cache, db, &mut images, &mut meshes, &mut materials, &mut glows, &mut panels, &mut parts, g.physicals[p].sprite_material, &mut textures, r, color, g.physicals[p].sprite_draw) else { continue };
        let key = sprite_key(g.physicals[p].sprite_layer, order, render_queue(db, g.physicals[p].sprite_material), bounds_z(&g.local_to_world(p), g.physicals[p].sprite_bounds.map_or([0.0; 3], |b| b.0)), 0, p);
        new.push((key, NewSprite::Object(p, scale, drawn, g.physicals[p].sprite_game_layer)));
    }
    for i in done.1..g.renderers.len() {
        let (r, color, order) = g.renderers[i].sprite;
        let Some((drawn, scale)) = make_sprite(&g.limb_sprite_cache, db, &mut images, &mut meshes, &mut materials, &mut glows, &mut panels, &mut parts, g.renderers[i].material, &mut textures, r, color, g.renderers[i].draw) else { continue };
        let key = sprite_key(g.renderers[i].layer, order, render_queue(db, g.renderers[i].material), g.anchored_world(g.renderers[i].anchor, &g.renderers[i].chain).0[2], 1, i);
        new.push((key, NewSprite::Renderer(i, scale, drawn, g.renderers[i].game_layer)));
    }
    if let Some(look) = &g.limb_status_look {
        for l in *statuses..g.limbs.len() {
            for (k, &(r, color, order, layer, draw)) in [look.frame, look.dead, look.bar].iter().enumerate() {
                let Some((drawn, scale)) = make_sprite(&g.limb_sprite_cache, db, &mut images, &mut meshes, &mut materials, &mut glows, &mut panels, &mut parts, None, &mut textures, r, color, draw) else { continue };
                let key = SortKey { layer: sorting_layer_index(layer), order, queue: 3000, distance: sort_distance(0.0, 0.0), group: 3, index: l * 3 + k };
                new.push((key, NewSprite::Status(l, k as u8, scale, drawn)));
            }
        }
        *statuses = g.limbs.len();
    }
    done = (g.physicals.len(), g.renderers.len());
    (play.done.1, play.done.2) = done;
    // limb_sprites and wires ran just before: what they added is ranked here too.
    if new.is_empty() && limbs_added.is_empty() && resorted.is_empty() {
        return;
    }

    let placed_now: Vec<(Entity, SortKey)> = placed.iter().map(|(e, k, _)| (e, *k)).collect();
    let mut keys: Vec<SortKey> = placed_now.iter().map(|&(_, k)| k).collect();
    keys.extend(new.iter().map(|(k, _)| *k));
    let depths = sprite_depths(&keys);
    let (old, fresh) = depths.split_at(placed_now.len());
    for ((entity, _), &depth) in placed_now.iter().zip(old) {
        if let Ok((_, _, mut transform)) = placed.get_mut(*entity) {
            transform.translation.z = depth;
        }
    }
    for ((key, sprite), &depth) in new.into_iter().zip(fresh) {
        match sprite {
            NewSprite::Object(p, scale, drawn, layer) => drawn.insert(commands.spawn((ObjectSprite(p, scale), Transform::from_xyz(0.0, 0.0, depth), key, crate::lights::camera_layers(layer))), play.game.physicals[p].sprite.unwrap().1),
            NewSprite::Renderer(i, scale, drawn, layer) => drawn.insert(commands.spawn((RendererSprite(i, scale), Transform::from_xyz(0.0, 0.0, depth), key, crate::lights::camera_layers(layer))), play.game.renderers[i].sprite.1),
            // The prefab's objects are on layer 16.
            NewSprite::Status(l, k, scale, drawn) => drawn.insert(commands.spawn((LimbStatusSprite(l, k, scale), Transform::from_xyz(0.0, 0.0, depth), Visibility::Hidden, key, crate::lights::camera_layers(16))), [f32::NAN; 4]),
        }
    }
}

/// How a SpriteRenderer reaches Bevy: a Simple one as a Sprite, a Sliced or Tiled one as the mesh Unity builds for it.
enum Drawn {
    Mesh(Handle<Mesh>, Handle<ColorMaterial>),
    /// With the material's _GlowColour and _GlowIntensity.
    Glow(Handle<Mesh>, Handle<crate::lights::Glow>, [f32; 4], f32),
    Panels(Handle<Mesh>, Handle<crate::lights::Panels>),
    /// Particles/Standard Unlit (VeryBright and the other glow materials): the renderer's colour is the vertex colour.
    Particle(Handle<Mesh>, Handle<crate::particles::ParticleMaterial>),
    /// A shader of custom_material.rs; its material is made (and kept current) by the sync system.
    Custom(Handle<Mesh>, crate::custom_material::CustomSprite),
}

/// Unlit/AdditiveGlowSprite's colour: everything but the texture.
fn glow_color(color: [f32; 4], g: [f32; 4], i: f32) -> Vec4 {
    let c = Color::srgba(color[0], color[1], color[2], color[3]).to_linear();
    Vec4::new(c.red * g[0], c.green * g[1], c.blue * g[2], c.alpha * g[3]) * (i * c.alpha)
}

/// The SpriteRenderer.color a sprite entity's material shows, and where it goes.
#[derive(Component)]
pub(crate) struct Tint {
    color: [f32; 4],
    /// The renderer's property block and material overrides as last applied (`RenderProps::version`).
    props: u32,
    to: Drawn,
}

impl Drawn {
    fn insert(self, mut entity: EntityCommands, color: [f32; 4]) {
        if let Drawn::Custom(mesh, sprite) = self {
            entity.insert((Mesh2d(mesh), sprite));
            return;
        }
        match &self {
            Drawn::Mesh(mesh, material) => entity.insert((Mesh2d(mesh.clone()), MeshMaterial2d(material.clone()))),
            Drawn::Glow(mesh, material, ..) => entity.insert((Mesh2d(mesh.clone()), MeshMaterial2d(material.clone()))),
            Drawn::Panels(mesh, material) => entity.insert((Mesh2d(mesh.clone()), MeshMaterial2d(material.clone()))),
            Drawn::Particle(mesh, material) => entity.insert((Mesh2d(mesh.clone()), MeshMaterial2d(material.clone()))),
            Drawn::Custom(..) => unreachable!(),
        };
        entity.insert(Tint { color, props: 0, to: self });
    }
}

/// SpriteRenderer.color set at run time (visual.rs and the light scripts): the sprite's material follows.
pub(crate) fn restyle(
    play: Res<Play>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut glows: ResMut<Assets<crate::lights::Glow>>,
    mut panels: ResMut<Assets<crate::lights::Panels>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tints: Query<(&mut Tint, Option<&ObjectSprite>, Option<&RendererSprite>)>,
) {
    let g = &play.game;
    for (mut tint, object, renderer) in &mut tints {
        let (color, at) = match (object, renderer) {
            (Some(&ObjectSprite(p, _)), _) => (g.physicals[p].sprite.map(|s| s.1), Some(crate::game::visual::SpriteAt::Physical(p))),
            (_, Some(&RendererSprite(i, _))) => (Some(g.renderers[i].sprite.1), Some(crate::game::visual::SpriteAt::Renderer(i))),
            _ => (None, None),
        };
        let props = at.and_then(|at| g.props.get(&at));
        let version = props.map_or(0, |p| p.version);
        let Some(c) = color.filter(|&c| c != tint.color || version != tint.props) else { continue };
        tint.color = c;
        tint.props = version;
        match &tint.to {
            Drawn::Mesh(_, h) => {
                if let Some(mut m) = materials.get_mut(h) {
                    m.color = Color::srgba(c[0], c[1], c[2], c[3]);
                }
            }
            Drawn::Glow(_, h, k, i) => {
                // _GlowColour and _GlowIntensity: the .mat's, then the scripts' material values and property block.
                let (mut k, mut i) = (*k, *i);
                for layer in props.iter().flat_map(|p| [&p.material, &p.block]) {
                    match layer.get("_GlowColour") {
                        Some(crate::game::props::Prop::Color(v) | crate::game::props::Prop::Vector(v)) => k = *v,
                        Some(other) => unimplemented!("mod-only: a {other:?} set on _GlowColour"),
                        None => {}
                    }
                    match layer.get("_GlowIntensity") {
                        Some(crate::game::props::Prop::Float(v)) => i = *v,
                        Some(other) => unimplemented!("mod-only: a {other:?} set on _GlowIntensity"),
                        None => {}
                    }
                }
                if let Some(mut m) = glows.get_mut(h) {
                    m.color = glow_color(c, k, i);
                }
            }
            Drawn::Panels(_, h) => {
                if let Some(mut m) = panels.get_mut(h) {
                    m.settings.alpha = c[3];
                }
            }
            Drawn::Particle(h, _) => {
                if let Some(mut m) = meshes.get_mut(h) {
                    vertex_colour(&mut m, c);
                }
            }
            // Never a Tint (Drawn::insert): custom_sync reads the colour itself.
            Drawn::Custom(..) => {}
        }
    }
}

/// A sprite's input to Unity's PrepareSpriteTilingData.
struct Tiling {
    size: [f32; 2],
    /// SpriteDrawMode: Sliced 1, Tiled 2.
    draw_mode: i32,
    adaptive: bool,
    threshold: f32,
    /// Sprite.rect's width and height in pixels.
    rect: [f32; 2],
    pivot: [f32; 2],
    /// left, bottom, right, top in pixels.
    border: [f32; 4],
    pixels_to_units: f32,
    /// uvMin.xy, uvMax.xy.
    uv: [f32; 4],
}

/// GetSourceAndDestinationRect on one axis: the (source start, source length, destination start, destination length)
/// of the near border, the middle and the far border, without those that are empty.
fn nine_slice_spans(near: f32, far: f32, rect: f32, length: f32) -> Vec<[f32; 4]> {
    let both = near + far;
    let scale = if length < both { length / both } else { 1.0 };
    let (mut source, mut destination, mut spans) = (0.0, 0.0, Vec::new());
    for [s, d] in [[near, near * scale], [rect - both, length - both], [far, far * scale]] {
        if !(s <= 0.0 || d <= 0.0) {
            spans.push([source, s, destination, d]);
        }
        source += if s < 0.0 { 0.0 } else { s };
        destination += if d < 0.0 { 0.0 } else { d };
    }
    spans
}

/// One axis of a section's whole tiles at Adaptive: below the threshold the last fraction stretches the others.
fn adaptive_tiles(area: f32, tile: f32, threshold: f32) -> f32 {
    let ratio = area / tile;
    let nearest = ratio.round_ties_even();
    let whole = nearest - if ratio < nearest { 1.0 } else { 0.0 };
    let frac = ratio - whole;
    f32::max(1.0, whole + if tile * threshold < frac { 1.0 } else { 0.0 })
}

/// The quads of PrepareSpriteTilingData + GenerateSpriteTileMesh, each four (position, uv) in Unity's sprite space.
fn tile_quads(t: &Tiling) -> Vec<[([f32; 2], [f32; 2]); 4]> {
    let units = 1.0 / t.pixels_to_units;
    let rect = t.rect.map(|v| v * units);
    let b = t.border.map(|v| v * units);
    let origin = [0, 1].map(|i| -(t.rect[i] * t.pivot[i]) * t.size[i] / t.rect[i]);
    let across = nine_slice_spans(b[0], b[2], rect[0], t.size[0].abs());
    let up = nine_slice_spans(b[1], b[3], rect[1], t.size[1].abs());
    let at = [0, 1].map(|i| origin[i] + if t.size[i] < 0.0 { t.size[i] } else { 0.0 });
    // Rows from the bottom, each from the left: (uv offset, tile, position, area).
    let sections: Vec<[[f32; 2]; 4]> = up
        .iter()
        .flat_map(|y| across.iter().map(move |x| (x, y)))
        .map(|(x, y)| [[x[0], y[0]], [x[1], y[1]], [x[2] + at[0], y[2] + at[1]], [x[3], y[3]]])
        .collect();
    let (mut verts, mut indices) = (0i32, 0i32);
    for [_, tile, _, area] in &sections {
        if t.draw_mode != 2 {
            verts += 4;
            indices += 6;
            continue;
        }
        let count = [0, 1].map(|i| if t.adaptive { adaptive_tiles(area[i], tile[i], t.threshold) } else { (area[i] / tile[i]).ceil() });
        let product = count[0] * count[1];
        verts = (product * 4.0 + verts as f32) as i32;
        indices = (product * 6.0 + indices as f32) as i32;
    }
    let (uv_min, uv_max) = ([t.uv[0], t.uv[1]], [t.uv[2], t.uv[3]]);
    // Too big for one mesh, or nothing at all: the plain quad.
    if verts <= 0 || verts >= 0xffff || indices >= 0x2fffd {
        let max = [origin[0] + t.size[0], origin[1] + t.size[1]];
        return vec![[(origin, uv_min), ([max[0], origin[1]], [uv_max[0], uv_min[1]]), (max, uv_max), ([origin[0], max[1]], [uv_min[0], uv_max[1]])]];
    }
    let span = [uv_max[0] - uv_min[0], uv_max[1] - uv_min[1]];
    let uv_of = |v: f32, i: usize| v * (1.0 / rect[i]) * span[i] + [uv_min[0], uv_min[1]][i];
    let mut quads = Vec::new();
    for [offset, tile, position, area] in &sections {
        let (count, used) = if t.draw_mode != 2 {
            ([1.0, 1.0], *area)
        } else if t.adaptive {
            let count = [0, 1].map(|i| adaptive_tiles(area[i], tile[i], t.threshold));
            (count, [area[0] / count[0], area[1] / count[1]])
        } else {
            ([area[0] / tile[0], area[1] / tile[1]], *tile)
        };
        let (u0, v0) = (uv_of(offset[0], 0), uv_of(offset[1], 1));
        let (u1, v1) = (uv_of(offset[0] + tile[0], 0), uv_of(offset[1] + tile[1], 1));
        let mut y = 0;
        while count[1] > y as f32 {
            let yf = y as f32;
            let h = if count[1] - yf > 1.0 || t.adaptive { 1.0 } else { count[1] - yf };
            let top = (1.0 - h) * v0 + h * v1;
            let mut x = 0;
            while count[0] > x as f32 {
                let xf = x as f32;
                let w = if count[0] - xf > 1.0 || t.adaptive { 1.0 } else { count[0] - xf };
                let right = (1.0 - w) * u0 + w * u1;
                let (x2, y2) = (w + xf, h + yf);
                let at = |x: f32, y: f32| [x * used[0] + position[0], y * used[1] + position[1]];
                quads.push([(at(xf, yf), [u0, v0]), (at(x2, yf), [right, v0]), (at(x2, y2), [right, top]), (at(xf, y2), [u0, top])]);
                x += 1;
            }
            y += 1;
        }
    }
    quads
}

/// The Bevy mesh of [`tile_quads`], mirrored by the renderer's flips about the pivot as the sprite shader's _Flip does.
/// A Simple sprite's own mesh (its RenderData: a Tight or polygon outline, not only the rect's quad).
fn sprite_mesh(s: &crate::assets::components::Sprite, flip: [bool; 2]) -> Mesh {
    let sign = flip.map(|f| if f { -1.0 } else { 1.0 });
    let (p, uv, index) = s.mesh();
    let p: Vec<[f32; 3]> = p.iter().map(|v| [v[0] * sign[0], v[1] * sign[1], 0.0]).collect();
    let mut indices = Vec::new();
    for t in index.chunks_exact(3) {
        let [a, b, c] = [p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]];
        // Mesh2d culls back faces; Unity's sprites draw both.
        let front = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) >= 0.0;
        indices.extend(if front { [t[0], t[1], t[2]] } else { [t[0], t[2], t[1]] });
    }
    // Kept in the main world too: the portal doorway (extras_view.rs) clips sprite meshes.
    let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, p);
    // Unity's v runs up; an Image's down.
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv.iter().map(|v| [v[0], 1.0 - v[1]]).collect::<Vec<_>>());
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    mesh
}

fn tile_mesh(t: &Tiling, flip: [bool; 2]) -> Mesh {
    let sign = flip.map(|f| if f { -1.0 } else { 1.0 });
    let (mut positions, mut uvs, mut indices) = (Vec::new(), Vec::new(), Vec::new());
    for quad in tile_quads(t) {
        let first = positions.len() as u32;
        let p: Vec<[f32; 3]> = quad.iter().map(|(p, _)| [p[0] * sign[0], p[1] * sign[1], 0.0]).collect();
        // Mesh2d culls back faces; Unity's sprites draw both.
        let front = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]) >= 0.0;
        indices.extend(if front { [0, 1, 2, 0, 2, 3] } else { [0, 2, 1, 0, 3, 2] }.map(|i| first + i));
        positions.extend(p);
        // Unity's v runs up; an Image's down.
        uvs.extend(quad.iter().map(|(_, uv)| [uv[0], 1.0 - uv[1]]));
    }
    // Kept in the main world too: the portal doorway (extras_view.rs) clips sprite meshes.
    let mut mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    mesh
}

/// What Bevy draws for a Unity Sprite: its packed region, colour, flips and draw mode, the anchor at its pivot, and
/// the scale from its units to the world.
/// A Texture2D as an Image, with its Unity filter and wrap modes.
pub fn load_texture(db: &crate::assets::db::AssetDb, images: &mut Assets<Image>, textures: &mut Textures, t: crate::assets::db::ObjRef) -> Option<(Handle<Image>, [f32; 2])> {
    if let Some(found) = textures.get(&(t.file, t.path_id)) {
        return Some(found.clone());
    }
    let tt = std::time::Instant::now();
    let tex = crate::assets::components::Texture2D::read(db.data(t));
    let Ok((tex, rgba)) = tex.and_then(|tex| tex.rgba(&db.data_dir).map(|px| (tex, px))) else { return None };
    if std::env::var_os("PP_SPAWN_BENCH").is_some() { eprintln!("BENCH     texture {:?} {t:?} {}x{} decode {:.2} ms", tex.name, tex.width, tex.height, tt.elapsed().as_secs_f64() * 1000.0); }
    Some(upload_texture(images, textures, t, &tex, &rgba))
}

/// A decoded Texture2D (`rgba` bottom row first) as an Image, with its Unity filter and wrap modes.
fn upload_texture(images: &mut Assets<Image>, textures: &mut Textures, t: crate::assets::db::ObjRef, tex: &crate::assets::components::Texture2D, rgba: &[u8]) -> (Handle<Image>, [f32; 2]) {
    use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    // Unity's rows run bottom first; an Image's top first.
    let row = tex.width as usize * 4;
    let flipped: Vec<u8> = rgba.chunks(row).rev().flatten().copied().collect();
    let mut image = Image::new(
        bevy::render::render_resource::Extent3d { width: tex.width as u32, height: tex.height as u32, depth_or_array_layers: 1 },
        bevy::render::render_resource::TextureDimension::D2,
        flipped,
        // ColorSpace: 0 for a texture imported as plain data, as DamageMap is.
        if tex.color_space == 0 { bevy::render::render_resource::TextureFormat::Rgba8Unorm } else { bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb },
        bevy::asset::RenderAssetUsages::default(),
    );
    let filter = if tex.filter_mode == 0 { ImageFilterMode::Nearest } else { ImageFilterMode::Linear };
    // TextureWrapMode: 0 Repeat, 1 Clamp, 2 Mirror, 3 MirrorOnce.
    let wrap = |w: i32| match w {
        0 => ImageAddressMode::Repeat,
        1 => ImageAddressMode::ClampToEdge,
        2 => ImageAddressMode::MirrorRepeat,
        w => unimplemented!("mod-only: TextureWrapMode {w} (MirrorOnce): no sampler address mode matches it"),
    };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: wrap(tex.wrap[0]),
        address_mode_v: wrap(tex.wrap[1]),
        mag_filter: filter,
        min_filter: filter,
        ..default()
    });
    let entry = (images.add(image), [tex.width as f32, tex.height as f32]);
    textures.insert((t.file, t.path_id), entry.clone());
    entry
}

/// Unlit/Jaap/PanelsShader's material values with `main` as _MainTex and the SpriteRenderer colour's `alpha`: Some(None) for
/// a material of another shader, None when the material or one of the shader's values cannot be read.
pub(crate) fn panels_material(db: &crate::assets::db::AssetDb, images: &mut Assets<Image>, textures: &mut Textures, material: crate::assets::db::ObjRef, main: Handle<Image>, alpha: f32) -> Option<Option<crate::lights::Panels>> {
    let file = material.file;
    let m = crate::assets::components::Material::read(db.data(material)).ok()?;
    if db.material_shader_id(file, m.shader) != PANELS_SHADER {
        return Some(None);
    }
    let (atlas, size) = load_texture(db, images, textures, db.resolve(file, m.textures.iter().find(|(n, ..)| n == "_Panels")?.1)?)?;
    let panel = m.float("_PanelSize")?;
    let [a, b, ..] = m.color("_Offset")?;
    let [c, d, ..] = m.color("_ScrollingSpeed")?;
    let [fr, fg, fb, fa] = m.color("_FadeColour")?;
    let settings = crate::lights::PanelSettings {
        offset: Vec2::new(a, b),
        scrolling_speed: Vec2::new(c, d),
        fade: Color::srgba(fr, fg, fb, fa).to_linear().to_vec4(),
        panels: Vec2::new(size[0] / panel, size[1] / panel),
        scale: m.float("_Scale")?,
        colour_variation: m.float("_ColourVariation")?,
        alpha,
        time: 0.0,
    };
    Some(Some(crate::lights::Panels { settings, main, panels: atlas }))
}

fn make_sprite(cache: &crate::game::limb_sprites::LimbSpriteCache, db: &crate::assets::db::AssetDb, images: &mut Assets<Image>, meshes: &mut Assets<Mesh>, materials: &mut Assets<ColorMaterial>, glows: &mut Assets<crate::lights::Glow>, panels: &mut Assets<crate::lights::Panels>, parts: &mut Assets<crate::particles::ParticleMaterial>, material: Option<crate::assets::db::ObjRef>, textures: &mut Textures, r: crate::assets::db::ObjRef, color: [f32; 4], draw: crate::game::SpriteDraw) -> Option<(Drawn, f32)> {
    {
        // A sprite LimbSpriteCache made (SetBodyTextures): the whole texture region about its centre, Simple.
        if r.file == crate::game::limb_sprites::RUNTIME_SPRITE_FILE {
            let c = &cache.generated[r.path_id as usize];
            if draw.1 != 0 {
                unimplemented!("mod-only: a SpriteRenderer in a Sliced or Tiled draw mode showing a LimbSpriteCache sprite");
            }
            let (handle, texture) = load_texture(db, images, textures, c.texture)?;
            let [x, y, w, h] = c.rect;
            let ppu = c.pixels_per_unit;
            let tiling = Tiling { size: [w / ppu, h / ppu], draw_mode: 1, adaptive: false, threshold: 0.0, rect: [w, h], pivot: [0.5, 0.5], border: [0.0; 4], pixels_to_units: ppu, uv: [x / texture[0], y / texture[1], (x + w) / texture[0], (y + h) / texture[1]] };
            let linear = Color::srgba(color[0], color[1], color[2], color[3]);
            let material = materials.add(ColorMaterial { color: linear, texture: Some(handle), ..default() });
            return Some((Drawn::Mesh(meshes.add(tile_mesh(&tiling, draw.0)), material), 1.0));
        }
        let s = match db.sprite(r) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("sprite {r:?}: {e}");
                return None;
            }
        };
        let t = db.resolve(r.file, s.texture)?;
        let (handle, texture) = load_texture(db, images, textures, t)?;
        // The packed region (texture_rect) sits at texture_rect_offset inside the sprite's rect.
        let [x, y, w, h] = s.texture_rect;
        let linear = Color::srgba(color[0], color[1], color[2], color[3]);
        let ([flip_x, flip_y], draw_mode, size, threshold, adaptive) = draw;
        // Unlit/AdditiveGlowSprite: `color` of the Glow is everything but the texture.
        let glow = material.and_then(|m| {
            let file = m.file;
            let m = crate::assets::components::Material::read(db.data(m)).ok()?;
            let is_glow = db.material_shader_id(file, m.shader) == ADDITIVE_GLOW_SHADER;
            is_glow.then(|| (m.color("_GlowColour").unwrap_or([1.0; 4]), m.float("_GlowIntensity").unwrap_or(1.0)))
        });
        // Unlit/Jaap/PanelsShader, on a Simple sprite of the map.
        if let Some(m) = material {
            if let Some(p) = panels_material(db, images, textures, m, handle.clone(), color[3])? {
                let pivot = [(s.pivot[0] * s.rect[2] - s.texture_rect_offset[0]) / w, (s.pivot[1] * s.rect[3] - s.texture_rect_offset[1]) / h];
                let uv = [x / texture[0], y / texture[1], (x + w) / texture[0], (y + h) / texture[1]];
                let tiling = Tiling { size: [w / s.pixels_to_units, h / s.pixels_to_units], draw_mode: 1, adaptive, threshold, rect: [w, h], pivot, border: [0.0; 4], pixels_to_units: s.pixels_to_units, uv };
                return Some((Drawn::Panels(meshes.add(tile_mesh(&tiling, [flip_x, flip_y])), panels.add(p)), 1.0));
            }
        }
        // A custom shader (custom_material.rs): the sprite's mesh, and the material's values and the scripts' property block on top.
        if let Some(m) = material {
            let file = m.file;
            let m = crate::assets::components::Material::read(db.data(m)).ok()?;
            if let Some(def) = crate::custom_material::def_for(db.material_shader_id(file, m.shader)) {
                let [u, v] = [(x - s.texture_rect_offset[0]) / texture[0], (y - s.texture_rect_offset[1]) / texture[1]];
                let uv = [u, v, u + s.rect[2] / texture[0], v + s.rect[3] / texture[1]];
                let draw_mode_given = draw_mode;
                let (draw_mode, size, border) = if draw_mode == 0 { (1, [s.rect[2] / s.pixels_to_units, s.rect[3] / s.pixels_to_units], [0.0; 4]) } else { (draw_mode, size, s.border) };
                let tiling = Tiling { size, draw_mode, adaptive, threshold, rect: [s.rect[2], s.rect[3]], pivot: s.pivot, border, pixels_to_units: s.pixels_to_units, uv };
                let sprite = crate::custom_material::new_sprite(def, db, images, textures, file, &m, handle, uv_to_local(&tiling, [flip_x, flip_y]))?;
                // A Simple sprite outside an atlas draws its own mesh (the shader may alpha-test, as Jaap/ForcefieldShader does).
                // Some sprites ship a zeroed TexCoord0 stream (Jukebox_0), or one of ~1e-38 junk (Square, the water's): those draw their
                // rect's quad, the uvs from the rect. No real uv is under a texel of a 16384 texture.
                let own = draw_mode_given == 0 && s.atlas.is_null() && s.mesh().1.iter().any(|uv| uv[0].abs().max(uv[1].abs()) > 1e-6);
                let mesh = if own { sprite_mesh(&s, [flip_x, flip_y]) } else { tile_mesh(&tiling, [flip_x, flip_y]) };
                return Some((Drawn::Custom(meshes.add(mesh), sprite), 1.0));
            }
        }
        // Particles/Standard Unlit (VeryBright, SpriteLightFlash, Death, Vapor): _MainTex * _Color * the renderer's colour as the
        // vertex colour, in the material's blend mode (VeryBright's Additive adds a black texel as nothing).
        if let Some(mr) = material {
            let m = crate::assets::components::Material::read(db.data(mr)).ok()?;
            if db.material_shader_id(mr.file, m.shader) == PARTICLES_SHADER {
                let tiling = if draw_mode == 0 {
                    // A Simple sprite: the packed region's quad, about the sprite's pivot.
                    let pivot = [(s.pivot[0] * s.rect[2] - s.texture_rect_offset[0]) / w, (s.pivot[1] * s.rect[3] - s.texture_rect_offset[1]) / h];
                    let uv = [x / texture[0], y / texture[1], (x + w) / texture[0], (y + h) / texture[1]];
                    Tiling { size: [w / s.pixels_to_units, h / s.pixels_to_units], draw_mode: 1, adaptive, threshold, rect: [w, h], pivot, border: [0.0; 4], pixels_to_units: s.pixels_to_units, uv }
                } else {
                    let [u, v] = [(x - s.texture_rect_offset[0]) / texture[0], (y - s.texture_rect_offset[1]) / texture[1]];
                    let uv = [u, v, u + s.rect[2] / texture[0], v + s.rect[3] / texture[1]];
                    Tiling { size, draw_mode, adaptive, threshold, rect: [s.rect[2], s.rect[3]], pivot: s.pivot, border: s.border, pixels_to_units: s.pixels_to_units, uv }
                };
                let mut mesh = tile_mesh(&tiling, [flip_x, flip_y]);
                // particle.wesl takes Unity's v (up).
                if let Some(bevy::mesh::VertexAttributeValues::Float32x2(uvs)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
                    uvs.iter_mut().for_each(|uv| uv[1] = 1.0 - uv[1]);
                }
                let n = mesh.count_vertices();
                mesh.insert_attribute(crate::particles::UV2_BLEND, vec![[0.0f32; 3]; n]);
                vertex_colour(&mut mesh, color);
                let st = m.textures.iter().find(|(n, ..)| n == "_MainTex").map_or([1.0, 1.0, 0.0, 0.0], |t| t.2);
                return Some((Drawn::Particle(meshes.add(mesh), parts.add(crate::particles::particle_material(&m, st, handle, false))), 1.0));
            }
        }
        if let Some((g, i)) = glow {
            let color = glow_color(color, g, i);
            let [u, v] = [(x - s.texture_rect_offset[0]) / texture[0], (y - s.texture_rect_offset[1]) / texture[1]];
            let uv = [u, v, u + s.rect[2] / texture[0], v + s.rect[3] / texture[1]];
            // A Simple sprite is the quad of a Sliced one without borders.
            let (draw_mode, size, border) = if draw_mode == 0 { (1, [s.rect[2] / s.pixels_to_units, s.rect[3] / s.pixels_to_units], [0.0; 4]) } else { (draw_mode, size, s.border) };
            let tiling = Tiling { size, draw_mode, adaptive, threshold, rect: [s.rect[2], s.rect[3]], pivot: s.pivot, border, pixels_to_units: s.pixels_to_units, uv };
            return Some((Drawn::Glow(meshes.add(tile_mesh(&tiling, [flip_x, flip_y])), glows.add(crate::lights::Glow { color, texture: handle }), g, i), 1.0));
        }
        if draw_mode != 0 {
            // The rect's corner in the texture: the packed region less its offset.
            let [u, v] = [(x - s.texture_rect_offset[0]) / texture[0], (y - s.texture_rect_offset[1]) / texture[1]];
            let uv = [u, v, u + s.rect[2] / texture[0], v + s.rect[3] / texture[1]];
            let tiling = Tiling { size, draw_mode, adaptive, threshold, rect: [s.rect[2], s.rect[3]], pivot: s.pivot, border: s.border, pixels_to_units: s.pixels_to_units, uv };
            let mesh = meshes.add(tile_mesh(&tiling, [flip_x, flip_y]));
            let material = materials.add(ColorMaterial { color: linear, texture: Some(handle), ..default() });
            return Some((Drawn::Mesh(mesh, material), 1.0));
        }
        // A Simple sprite: the packed region's quad, about the sprite's pivot.
        let rect = [w, h];
        let pivot = [(s.pivot[0] * s.rect[2] - s.texture_rect_offset[0]) / w, (s.pivot[1] * s.rect[3] - s.texture_rect_offset[1]) / h];
        let uv = [x / texture[0], y / texture[1], (x + w) / texture[0], (y + h) / texture[1]];
        let tiling = Tiling { size: [w / s.pixels_to_units, h / s.pixels_to_units], draw_mode: 1, adaptive, threshold, rect, pivot, border: [0.0; 4], pixels_to_units: s.pixels_to_units, uv };
        let material = materials.add(ColorMaterial { color: linear, texture: Some(handle), ..default() });
        Some((Drawn::Mesh(meshes.add(tile_mesh(&tiling, [flip_x, flip_y])), material), 1.0))
    }
}

/// A new Simple, unflipped SpriteRenderer's mesh of sprite `r` (the packed region's quad about the pivot), its texture,
/// the texture's size and the sprite's lowest uv and rect over the texture size (`_AtlasTransform`).
pub(crate) fn simple_sprite(cache: &crate::game::limb_sprites::LimbSpriteCache, db: &crate::assets::db::AssetDb, images: &mut Assets<Image>, textures: &mut Textures, r: crate::assets::db::ObjRef) -> Option<(Mesh, Handle<Image>, [f32; 2], [f32; 4])> {
    // A LimbSpriteCache sprite (Sprite.Create): the whole rect about its centre, nothing packed.
    if r.file == crate::game::limb_sprites::RUNTIME_SPRITE_FILE {
        let c = &cache.generated[r.path_id as usize];
        let (handle, texture) = load_texture(db, images, textures, c.texture)?;
        let [x, y, w, h] = c.rect;
        let uv = [x / texture[0], y / texture[1], (x + w) / texture[0], (y + h) / texture[1]];
        let tiling = Tiling { size: [w / c.pixels_per_unit, h / c.pixels_per_unit], draw_mode: 1, adaptive: false, threshold: 0.0, rect: [w, h], pivot: [0.5, 0.5], border: [0.0; 4], pixels_to_units: c.pixels_per_unit, uv };
        return Some((tile_mesh(&tiling, [false, false]), handle, texture, [uv[0], uv[1], w / texture[0], h / texture[1]]));
    }
    let s = db.sprite(r).ok()?;
    let (handle, texture) = load_texture(db, images, textures, db.resolve(r.file, s.texture)?)?;
    let [x, y, w, h] = s.texture_rect;
    let pivot = [(s.pivot[0] * s.rect[2] - s.texture_rect_offset[0]) / w, (s.pivot[1] * s.rect[3] - s.texture_rect_offset[1]) / h];
    let uv = [x / texture[0], y / texture[1], (x + w) / texture[0], (y + h) / texture[1]];
    let tiling = Tiling { size: [w / s.pixels_to_units, h / s.pixels_to_units], draw_mode: 1, adaptive: false, threshold: 0.0, rect: [w, h], pivot, border: [0.0; 4], pixels_to_units: s.pixels_to_units, uv };
    // The sprite's own mesh outside an atlas, as for the custom shaders (junk or zeroed uvs draw the rect's quad); the atlas
    // origin is Utils.GetMin(sprite.uv, sqrMagnitude): the first uv of least length, so of the drawn mesh.
    let (_, uvs, _) = s.mesh();
    let own = s.atlas.is_null() && uvs.iter().any(|uv| uv[0].abs().max(uv[1].abs()) > 1e-6);
    let min = if own { uvs.iter().fold((f32::MAX, [0.0; 2]), |m, v| { let d = v[0] * v[0] + v[1] * v[1]; if d < m.0 { (d, *v) } else { m } }).1 } else { [uv[0], uv[1]] };
    let atlas = [min[0], min[1], s.rect[2] / texture[0], s.rect[3] / texture[1]];
    Some((if own { sprite_mesh(&s, [false, false]) } else { tile_mesh(&tiling, [false, false]) }, handle, texture, atlas))
}

/// The (scale, offset) from uv to a limb sprite's vertex positions the body shaders are given.
fn uv_to_local(tiling: &Tiling, flip: [bool; 2]) -> Vec4 {
    let quad = tile_quads(tiling)[0];
    let ([ax, ay], [au, av]) = quad[0];
    let ([bx, by], [bu, bv]) = quad[2];
    let sign = flip.map(|f| if f { -1.0 } else { 1.0 });
    // The mesh's v is 1 - v.
    let (sx, sy) = ((bx - ax) / (bu - au) * sign[0], (by - ay) / (av - bv) * sign[1]);
    Vec4::new(sx, sy, ax * sign[0] - au * sx, ay * sign[1] - (1.0 - av) * sy)
}

/// A sprite for each limb without one: the starting people's, then the ones spawned since.
#[allow(clippy::too_many_arguments)]
pub(crate) fn limb_sprites(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut bodies: ResMut<Assets<crate::limb_material::Body>>,
    mut legacies: ResMut<Assets<crate::limb_material::LegacyBody>>,
    mut buffers: ResMut<Assets<bevy::render::storage::ShaderBuffer>>,
    mut textures: ResMut<Textures>,
    mut swaps: Query<(&LimbSprite, &mut Mesh2d, &mut Swapped, Option<&crate::limb_material::LimbMaterial>, Option<&MeshMaterial2d<ColorMaterial>>)>,
    mut play: ResMut<Play>,
) {
    use crate::{assets::components::{Material, Sprite as UnitySprite}, limb_material as lm};
    let g = &play.game;
    let Some(db) = g.assets.as_deref() else { return };
    // PersonBehaviour.SetBodyTextures: the limbs whose sprite or textures changed.
    for (&LimbSprite(l), mut mesh, mut swapped, material, plain) in &mut swaps {
        let k = &g.skins[l];
        if swapped.0 == k.version {
            continue;
        }
        swapped.0 = k.version;
        let Some(s) = k.sprite else { continue };
        let c = &g.limb_sprite_cache.generated[s];
        let (main, atlas) = load_texture(db, &mut images, &mut textures, c.texture).unwrap();
        let [x, y, w, h] = c.rect;
        // Sprite.Create's FullRect sprite about its centre.
        let tiling = Tiling { size: [w / c.pixels_per_unit, h / c.pixels_per_unit], draw_mode: 1, adaptive: false, threshold: 0.0, rect: [w, h], pivot: [0.5, 0.5], border: c.border, pixels_to_units: c.pixels_per_unit, uv: [x / atlas[0], y / atlas[1], (x + w) / atlas[0], (y + h) / atlas[1]] };
        let flip = g.physicals[g.limbs[l].PhysicalBehaviour].sprite_draw.0;
        mesh.0 = meshes.add(tile_mesh(&tiling, flip));
        let mut other = |t: Option<crate::assets::db::ObjRef>| t.map(|t| load_texture(db, &mut images, &mut textures, t).unwrap().0);
        let (flesh, bone) = (other(k.flesh_texture), other(k.bone_texture));
        let uv = uv_to_local(&tiling, flip);
        match material {
            Some(lm::LimbMaterial::Body(h)) => {
                if let Some(mut m) = bodies.get_mut(h) {
                    m.set_textures(main, flesh, bone);
                    m.extra.uv_to_local = uv;
                }
            }
            Some(lm::LimbMaterial::Legacy(h)) => {
                if let Some(mut m) = legacies.get_mut(h) {
                    m.set_textures(main, flesh, bone);
                    m.extra.uv_to_local = uv;
                }
            }
            None => {
                if let Some(mut m) = plain.and_then(|p| materials.get_mut(&p.0)) {
                    m.texture = Some(main);
                }
            }
        }
    }
    for l in play.done.0..g.limbs.len() {
        let limb = &g.limbs[l];
        let ph = &g.physicals[limb.PhysicalBehaviour];
        let Some((sprite, color, order)) = ph.sprite else { unimplemented!("mod-only: limb {} without a SpriteRenderer", ph.name) };
        let s = db.sprite(sprite).unwrap();
        let (handle, atlas) = load_texture(db, &mut images, &mut textures, db.resolve(sprite.file, s.texture).unwrap()).unwrap();
        // SpriteRenderer's Simple quad of the packed region, as make_sprite draws it.
        let [x, y, w, h] = s.texture_rect;
        let pivot = [(s.pivot[0] * s.rect[2] - s.texture_rect_offset[0]) / w, (s.pivot[1] * s.rect[3] - s.texture_rect_offset[1]) / h];
        let uv = [x / atlas[0], y / atlas[1], (x + w) / atlas[0], (y + h) / atlas[1]];
        let tiling = Tiling { size: [w / s.pixels_to_units, h / s.pixels_to_units], draw_mode: 1, adaptive: false, threshold: 0.0, rect: [w, h], pivot, border: [0.0; 4], pixels_to_units: s.pixels_to_units, uv };
        let (flip, ..) = ph.sprite_draw;
        let mesh = meshes.add(tile_mesh(&tiling, flip));
        let linear = Color::srgba(color[0], color[1], color[2], color[3]);
        // Person.ChosenMaterial (PersonBehaviour.Awake) when it is set, else the renderer's own.
        let person = &g.people[limb.Person].materials;
        let chosen = if g.prefs.GorelessMode { person[2] } else { person[g.prefs.GoreMode as usize] }.or(ph.sprite_material);
        let key = sprite_key(ph.sprite_layer, order, render_queue(db, chosen), bounds_z(&g.local_to_world(limb.PhysicalBehaviour), ph.sprite_bounds.map_or([0.0; 3], |b| b.0)), 0, limb.PhysicalBehaviour);
        let body = chosen.and_then(|r| {
            let m = Material::read(db.data(r)).ok()?;
            let shader = db.material_shader_id(r.file, m.shader);
            (shader == lm::BODY_SHADER || shader == lm::LEGACY_BODY_SHADER).then_some((r, m, shader == lm::LEGACY_BODY_SHADER))
        });
        let mut entity = commands.spawn((LimbSprite(l), Swapped(0), Mesh2d(mesh), Transform::default(), key));
        if let Some((r, m, legacy)) = body {
            let mut texture = |name: &str| {
                let (_, t, _) = m.textures.iter().find(|(n, ..)| n == name).unwrap();
                load_texture(db, &mut images, &mut textures, db.resolve(r.file, *t).unwrap()).unwrap().0
            };
            let t = lm::Textures { flesh: texture("_FleshTex"), bone: texture("_BoneTex"), damage: texture("_DamageTex"), main: handle };
            let uv_to_local = uv_to_local(&tiling, flip);
            let extra = lm::Extra { tint: linear.to_linear().to_vec4(), uv_to_local };
            let material = lm::new_material(&m, legacy, extra, t, &mut bodies, &mut legacies, &mut buffers);
            match &material {
                lm::LimbMaterial::Body(h) => entity.insert(MeshMaterial2d(h.clone())),
                lm::LimbMaterial::Legacy(h) => entity.insert(MeshMaterial2d(h.clone())),
            };
            entity.insert((material, lm::BloodColour(g.original_blood_color(l))));
        } else {
            entity.insert(MeshMaterial2d(materials.add(ColorMaterial { color: linear, texture: Some(handle), ..default() })));
        }
    }
    play.done.0 = play.game.limbs.len();
}

pub(crate) fn draw(
    play: Res<Play>,
    settings: Res<crate::ui::Settings>,
    mut sprites: Query<(&LimbSprite, &mut Transform, &mut Visibility)>,
    mut objects: Query<(&ObjectSprite, &mut Transform, &mut Visibility), Without<LimbSprite>>,
    mut renderers: Query<(&RendererSprite, &mut Transform, &mut Visibility), (Without<LimbSprite>, Without<ObjectSprite>)>,
    mut statuses: Query<(&LimbStatusSprite, &mut Transform, &mut Visibility), (Without<LimbSprite>, Without<ObjectSprite>, Without<RendererSprite>)>,
    mut gizmos: Gizmos,
) {
    let g = &play.game;
    // LimbStatusBehaviour: unparented at the limb's position (ParentConstraint, no rotation) with its world scale;
    // Update shows BarSprite and the bar at health x blood flow, or DeadSprite.
    // LimbStatusBehaviour.Awake: drawn here (SetParent(null) keeps the world scale `status_scale` taken in limb_awake, SetActive(ShowLimbStatus): `shown`)
    // LimbStatusBehaviour.Start: drawn here (ParentConstraint source = the limb, position only: the sprites follow the limb's position, active or not)
    // LimbStatusBehaviour.Main_LimbStatusToggled: drawn here (SetActive(e) unless disintegrated: `shown` reads ShowLimbStatus and isDisintegrated)
    // LimbStatusBehaviour.PhysicalBehaviour_OnDisintegration: drawn here (SetActive(false): `shown` reads isDisintegrated)
    // LimbStatusBehaviour.OnDestroy: drawn here (LimbBehaviour.OnDestroy destroys the clone: `shown` reads destroyed)
    // LimbStatusBehaviour.constructor: field defaults (BarMaxWidth = 9)
    // LimbStatusBehaviour.Update is the sprite and bar below (Health / InitialHealth * BloodFlow * BarMaxWidth / 35).
    for (&LimbStatusSprite(l, part, k), mut transform, mut visibility) in &mut statuses {
        let limb = &g.limbs[l];
        let p = &g.physicals[limb.PhysicalBehaviour];
        let alive = g.limb_is_considered_alive(l);
        let shown = g.ShowLimbStatus && !p.destroyed && !p.isDisintegrated && (part == 1) != alive;
        *visibility = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if !shown {
            continue;
        }
        let [sx, sy] = limb.status_scale;
        let [x, y, _] = p.transform.position;
        let z = transform.translation.z;
        let w = g.limb_status_look.as_ref().map_or(9.0, |s| s.BarMaxWidth) / 35.0;
        let (dx, scale) = match part {
            2 => {
                let num = crate::game::mathf::clamp01(limb.Health / limb.InitialHealth) * crate::game::mathf::clamp01(g.circulations[l].BloodFlow) * w;
                ((num - w) / 2.0, Vec3::new(num * sx * k, sy / 35.0 * k, 1.0))
            }
            _ => (0.0, Vec3::new(sx * k, sy * k, 1.0)),
        };
        *transform = Transform::from_xyz(x + dx * sx, y, z).with_scale(scale);
    }
    for (&LimbSprite(l), mut transform, mut visibility) in &mut sprites {
        let limb = &g.limbs[l];
        let p = &g.physicals[limb.PhysicalBehaviour];
        let [sx, sy, _] = g.people[limb.Person].localScale;
        let [x, y, _] = p.transform.position;
        let [qx, qy, qz, qw] = p.transform.rotation;
        let z = transform.translation.z;
        *transform = Transform::from_xyz(x, y, z)
            .with_rotation(Quat::from_xyzw(qx, qy, qz, qw))
            .with_scale(Vec3::new(sx * p.transform.localScale[0], sy * p.transform.localScale[1], 1.0));
        *visibility = if p.active { Visibility::Inherited } else { Visibility::Hidden };
    }
    for (&ObjectSprite(pi, k), mut transform, mut visibility) in &mut objects {
        let p = &g.physicals[pi];
        *transform = object_sprite_transform(g, pi, transform.translation.z, k);
        *visibility = if p.active && !p.isDisintegrated && p.sprite_enabled { Visibility::Inherited } else { Visibility::Hidden };
    }
    for (&RendererSprite(i, k), mut transform, mut visibility) in &mut renderers {
        let r = &g.renderers[i];
        let ([x, y, _], [qx, qy, qz, qw], s) = g.anchored_world(r.anchor, &r.chain);
        let z = transform.translation.z;
        let mut t = Transform::from_xyz(x, y, z).with_rotation(Quat::from_xyzw(qx, qy, qz, qw)).with_scale(Vec3::new(s[0] * k, s[1] * k, 1.0));
        // A parent's non-uniform scale above a rotated Transform is not the product of the local scales.
        if let Some(m) = matrix_sprite_transform(&crate::unity::transform::local_to_world(&g.anchored_nodes(r.anchor, &r.chain)), z, k) {
            if (m.scale.x - t.scale.x.abs()).abs() > 1e-4 * m.scale.x.abs().max(1.0) || (m.scale.y.abs() - t.scale.y.abs()).abs() > 1e-4 * m.scale.y.abs().max(1.0) {
                t = m;
            }
        }
        *transform = t;
        let shown = match r.anchor {
            crate::game::BareAnchor::Physical(p) => g.physicals[p].active && !g.physicals[p].isDisintegrated,
            _ => true,
        };
        *visibility = if r.visible && r.enabled && shown { Visibility::Inherited } else { Visibility::Hidden };
    }
    // Everything else (sprite-less map parts and props) as collider outlines.
    let mut limb_bodies: Vec<_> = g.limbs.iter().map(|l| g.physicals[l.PhysicalBehaviour].rigidbody.body).collect();
    if g.assets.is_some() {
        limb_bodies.extend(g.physicals.iter().filter(|p| p.sprite.is_some()).map(|p| p.rigidbody.body));
    }
    // ModDebugRenderer.OnPostRender: ModdingApi.Draw's actions, last first, in magenta (GL lines or a line strip).
    for a in g.mod_debug_draw.take_for_render() {
        let v: Vec<Vec3> = a.vertices.iter().map(|p| Vec3::from_array(*p)).collect();
        if a.mode == 2 {
            gizmos.linestrip(v, Color::srgb(1.0, 0.0, 1.0));
        } else {
            for l in v.chunks_exact(2) {
                gizmos.line(l[0], l[1], Color::srgb(1.0, 0.0, 1.0));
            }
        }
    }
    for (id, body) in g.world.bodies.iter().enumerate() {
        let Some(body) = body else { continue };
        if limb_bodies.contains(&id) {
            continue;
        }
        let (p, q) = (body.xf.p, body.xf.q);
        let world = |v: crate::b2::math::Vec2| Vec2::new(q.c * v.x - q.s * v.y + p.x, q.s * v.x + q.c * v.y + p.y);
        for &f in &body.fixtures {
            // Extras: a portal's twin collider (extras_glue) is no map part.
            if g.world.portal_ghosts.contains(&f) {
                continue;
            }
            let color = Color::srgb(0.45, 0.5, 0.56);
            match &g.world.fixture(f).shape {
                Shape::Polygon(poly) => {
                    let points = poly.vertices[..poly.count].iter().chain(&poly.vertices[..1]).map(|&v| world(v));
                    gizmos.linestrip_2d(points, color);
                }
                Shape::Circle(c) => {
                    gizmos.circle_2d(world(c.p), c.radius, color);
                }
                Shape::Chain(c) => {
                    gizmos.linestrip_2d(c.vertices.iter().map(|&v| world(v)), color);
                }
                Shape::Capsule(c) => {
                    let [a, b] = c.vertices.map(world);
                    gizmos.circle_2d(a, c.radius, color);
                    gizmos.circle_2d(b, c.radius, color);
                    let n = (b - a).normalize_or_zero().perp() * c.radius;
                    gizmos.line_2d(a + n, b + n, color);
                    gizmos.line_2d(a - n, b - n, color);
                }
            }
        }
    }
}

/// An object's SpriteRenderer pose: its own TRS, or its localToWorldMatrix when a parent's scale changes it.
pub(crate) fn object_sprite_transform(g: &Game, pi: usize, z: f32, k: f32) -> Transform {
    let p = &g.physicals[pi];
    let [x, y, _] = p.transform.position;
    let [qx, qy, qz, qw] = p.transform.rotation;
    let s = p.transform.localScale;
    let t = Transform::from_xyz(x, y, z).with_rotation(Quat::from_xyzw(qx, qy, qz, qw)).with_scale(Vec3::new(s[0] * k, s[1] * k, 1.0));
    if p.parent.is_some() {
        if let Some(m) = matrix_sprite_transform(&g.local_to_world(pi), z, k) {
            if (m.scale.x - t.scale.x.abs()).abs() > 1e-4 * m.scale.x.abs().max(1.0) || (m.scale.y.abs() - t.scale.y.abs()).abs() > 1e-4 * m.scale.y.abs().max(1.0) {
                return m;
            }
        }
    }
    t
}

/// A SpriteRenderer's Transform.localToWorldMatrix (column-major) as a Bevy Transform of its xy part: the first column's
/// direction is the rotation, its length and the determinant over it the scales (a shear is dropped).
fn matrix_sprite_transform(m: &[f32; 16], z: f32, k: f32) -> Option<Transform> {
    let (a, b, c, d) = (m[0], m[1], m[4], m[5]);
    let sx = a.hypot(b);
    if sx < 1e-12 {
        return None;
    }
    let sy = (a * d - b * c) / sx;
    // The scale is returned positive in x and signed in y, so a mirrored matrix is a negative y scale.
    Some(Transform::from_xyz(m[12], m[13], z).with_rotation(Quat::from_rotation_z(b.atan2(a))).with_scale(Vec3::new(sx * k, sy * k, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RenderObjectSorter<0>: a sortingFudge puts a particle system before (behind) a sprite of the same layer, order and
    /// queue, a bigger fudge further back; with no fudge the sprite's group still comes first.
    #[test]
    fn sorting_fudge_draws_behind() {
        let sprite = SortKey { layer: 2, order: 0, queue: 3000, distance: sort_distance(0.0, 0.0), group: 0, index: 0 };
        assert!(particle_key(0, 0, 3000, 0.0, 2.0, 0) < sprite);
        assert!(particle_key(0, 0, 3000, 0.0, 25.0, 0) < particle_key(0, 0, 3000, 0.0, 2.0, 0));
        assert!(particle_key(0, 0, 3000, 0.0, 0.0, 0) > sprite);
        assert!(particle_key(0, 0, 3000, 0.0, -1.0, 0) > particle_key(0, 0, 3000, 0.0, 0.0, 0));
        // Farther from the camera (at z -10, looking +z) draws first.
        assert!(sprite_key(0, 0, 3000, 20.0, 0, 9) < sprite_key(0, 0, 3000, 1.0, 0, 0));
        assert!(sprite_key(0, 0, 3000, 0.0, 0, 0) == sprite);
    }

    /// Sorting layers are drawn in TagManager's order before sortingOrder: a Default-layer sprite with a higher order is
    /// still behind a Foreground one, and within a layer the lower order is behind.
    #[test]
    fn sprites_are_ranked_by_layer_then_order() {
        let key = |layer: i32, order: i16, index: usize| SortKey { layer: sorting_layer_index(layer), order, queue: 3000, distance: sort_distance(0.0, 0.0), group: 0, index };
        let foreground = 1377110059u32 as i32;
        let default_layer = 0;
        let keys = [key(foreground, 0, 0), key(default_layer, 100, 1), key(default_layer, 3, 2), key(default_layer, 5, 3)];
        let z = sprite_depths(&keys);
        assert!(z[1] < z[0], "the Default layer is behind Foreground");
        assert!(z[2] < z[3] && z[3] < z[1], "within a layer the lower sortingOrder is behind");
        assert!(z.iter().all(|&d| (-0.5..0.5).contains(&d)), "depths stay in the camera range: {z:?}");
        assert_eq!(sorting_layer_index(2607693843u32 as i32), 7, "Decals is the last layer");
    }

    /// A frame at Time.timeScale 0 still runs (the frame count moves) with deltaTime 0 and the game clock standing.
    #[test]
    fn a_zero_time_scale_frame_still_runs() {
        let mut g = Game::default();
        g.time.timeScale = 0.0;
        g.time.fixedDeltaTime = 0.02;
        let before = (g.time.timeAsDouble, g.time.fixedTimeAsDouble, g.time.frameCount);
        step(&mut g, 1.0 / 60.0).unwrap();
        assert_eq!((g.time.timeAsDouble, g.time.fixedTimeAsDouble), (before.0, before.1));
        assert_eq!((g.time.deltaTime, g.time.frameCount), (0.0, before.2 + 1));
        assert!(g.time.realtimeSinceStartup > 0.0);
    }
}

#[cfg(test)]
mod tiling_tests {
    use super::*;

    // A 4 by 4 unit sprite at 1 pixel per unit, centre pivot, no border, whole texture.
    fn tiled(size: [f32; 2], adaptive: bool, threshold: f32) -> Tiling {
        Tiling { size, draw_mode: 2, adaptive, threshold, rect: [4.0, 4.0], pivot: [0.5, 0.5], border: [0.0; 4], pixels_to_units: 1.0, uv: [0.0, 0.0, 1.0, 1.0] }
    }

    #[test]
    fn adaptive_stretches_below_the_threshold_and_adds_a_tile_above_it() {
        // ratio 1.25: the fraction .25 under threshold*tile 2.0 stretches one tile.
        assert_eq!(adaptive_tiles(5.0, 4.0, 0.5), 1.0);
        // The same fraction over threshold*tile 0.04 adds a second.
        assert_eq!(adaptive_tiles(5.0, 4.0, 0.01), 2.0);
        let stretched = tile_quads(&tiled([5.0, 4.0], true, 0.5));
        assert_eq!(stretched.len(), 1);
        assert_eq!(stretched[0][1], ([2.5, -2.0], [1.0, 0.0]));
    }

    #[test]
    fn continuous_clips_the_last_tile() {
        let q = tile_quads(&tiled([6.0, 4.0], false, 0.0));
        assert_eq!(q.len(), 2);
        assert_eq!(q[1][0].0, [1.0, -2.0]);
        assert_eq!(q[1][1], ([3.0, -2.0], [0.5, 0.0]));
    }

    #[test]
    fn nine_slice_keeps_borders_and_scales_them_when_too_small() {
        let mut t = tiled([10.0, 4.0], false, 0.0);
        t.draw_mode = 1;
        t.border = [1.0, 0.0, 2.0, 0.0];
        let q = tile_quads(&t);
        assert_eq!(q.len(), 3);
        assert_eq!((q[0][0].0[0], q[0][1].0[0]), (-5.0, -4.0));
        assert_eq!((q[1][0].0[0], q[1][1].0[0]), (-4.0, 3.0));
        assert_eq!((q[2][0].0[0], q[2][1].0[0]), (3.0, 5.0));
        assert_eq!((q[1][0].1[0], q[1][1].1[0]), (0.25, 0.5));
        // Drawn 1.5 wide: borders 1 and 2 scale by 1.5 / 3, the middle goes.
        assert_eq!(nine_slice_spans(1.0, 2.0, 4.0, 1.5), vec![[0.0, 1.0, 0.0, 0.5], [2.0, 2.0, 0.5, 1.0]]);
    }

    #[test]
    fn too_many_tiles_fall_back_to_one_quad() {
        let t = Tiling { rect: [1.0, 1.0], ..tiled([1000.0, 1000.0], false, 0.0) };
        assert_eq!(tile_quads(&t).len(), 1);
    }

    #[test]
    fn flips_mirror_about_the_pivot_and_stay_front_facing() {
        let mesh = tile_mesh(&tiled([4.0, 4.0], false, 0.0), [true, false]);
        let bevy::mesh::VertexAttributeValues::Float32x3(p) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap() else { panic!() };
        assert_eq!(p[0], [2.0, -2.0, 0.0]);
        let Some(bevy::mesh::Indices::U32(i)) = mesh.indices() else { panic!() };
        assert_eq!(i[..3], [0, 2, 1]);
    }
}

/// The window size: 1280x800, or PPG_SIZE=<w>x<h> (a rendering check at the real game's screenshot size).
fn shot_size() -> (u32, u32) {
    std::env::var("PPG_SIZE").ok().and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))).unwrap_or((1280, 800))
}

/// The sprite and material a SpriteRenderer entity was last built from.
#[derive(Component)]
pub(crate) struct Shown(crate::assets::db::ObjRef, Option<crate::assets::db::ObjRef>);

/// SpriteRenderer.sprite (TelevisionBehaviour's broken and night sprites) or sharedMaterial (DamagableMachineryBehaviour's broken
/// machine material) set at run time: the entity is built again from them, and its material's render queue sorts it.
pub(crate) fn resprite(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    mut glows: ResMut<Assets<crate::lights::Glow>>,
    mut panels: ResMut<Assets<crate::lights::Panels>>,
    mut parts: ResMut<Assets<crate::particles::ParticleMaterial>>,
    play: Res<Play>,
    mut textures: ResMut<Textures>,
    mut sprites: Query<(Entity, Option<&mut ObjectSprite>, Option<&mut RendererSprite>, Option<&mut Shown>, &mut SortKey)>,
) {
    let g = &play.game;
    let Some(db) = g.assets.as_deref() else { return };
    for (entity, object, renderer, shown, mut key) in &mut sprites {
        let (r, color, material, draw, layer, order) = match (&object, &renderer) {
            (Some(o), _) => {
                let p = &g.physicals[o.0];
                let Some((r, color, order)) = p.sprite else { continue };
                (r, color, p.sprite_material, p.sprite_draw, p.sprite_layer, order)
            }
            (_, Some(o)) => {
                let x = &g.renderers[o.0];
                (x.sprite.0, x.sprite.1, x.material, x.draw, x.layer, x.sprite.2)
            }
            _ => continue,
        };
        // sortingLayerID and sortingOrder set at run time (SortingLayerChild, the layer editor).
        resort(&mut key, layer, order);
        let Some(mut shown) = shown else {
            commands.entity(entity).insert(Shown(r, material));
            continue;
        };
        if (shown.0, shown.1) == (r, material) {
            continue;
        }
        (shown.0, shown.1) = (r, material);
        let Some((drawn, scale)) = make_sprite(&g.limb_sprite_cache, db, &mut images, &mut meshes, &mut materials, &mut glows, &mut panels, &mut parts, material, &mut textures, r, color, draw) else { continue };
        if let Some(mut o) = object {
            o.1 = scale;
        }
        if let Some(mut o) = renderer {
            o.1 = scale;
        }
        let queue = render_queue(db, material);
        if key.queue != queue {
            // object_sprites ranks the sprites again (Changed<SortKey>).
            key.queue = queue;
        }
        let mut e = commands.entity(entity);
        e.remove::<(Mesh2d, MeshMaterial2d<ColorMaterial>, MeshMaterial2d<crate::lights::Glow>, MeshMaterial2d<crate::lights::Panels>, MeshMaterial2d<crate::particles::ParticleMaterial>, Tint)>();
        crate::custom_material::remove(&mut e);
        drawn.insert(e, color);
    }
}

/// One television's VideoPlayer on the screen: the decoded clip and the Image it fills.
pub(crate) struct VideoScreen {
    clip: crate::assets::db::ObjRef,
    video: crate::video::Clip,
    image: Handle<Image>,
    serial: u64,
}

/// VideoPlayer.renderMode MaterialOverride on a television's ScreenRenderer: the clip the set plays, at its clock, is the
/// _MainTex of the screen's material (Particles, SrcAlpha One, tinted by _Color and the renderer's colour).
pub(crate) fn video_screens(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut glows: ResMut<Assets<crate::lights::Glow>>,
    play: Res<Play>,
    mut screens: Local<std::collections::HashMap<usize, VideoScreen>>,
    mut sprites: Query<(Entity, &RendererSprite, &mut Tint)>,
) {
    if play.is_added() {
        screens.clear();
    }
    let g = &play.game;
    let Some(db) = g.assets.as_deref() else { return };
    for (t, tv) in g.televisions.iter().enumerate() {
        let Some(crate::game::visual::SpriteAt::Renderer(r)) = tv.screen_renderer else { continue };
        if !g.renderers[r].enabled {
            continue;
        }
        let wanted = tv.clips[tv.video_clip].clip;
        if screens.get(&t).is_none_or(|s| s.clip != wanted) {
            let Ok(clip) = crate::assets::components::VideoClip::read(db.data(wanted)) else { continue };
            let video = crate::video::Clip::new(clip.webm(&db.data_dir).unwrap_or_else(|e| panic!("VideoClip {}: {e}", clip.name))).unwrap_or_else(|e| panic!("VideoClip {}: {e}", clip.name));
            let mut image = Image::new_fill(
                bevy::render::render_resource::Extent3d { width: video.width, height: video.height, depth_or_array_layers: 1 },
                bevy::render::render_resource::TextureDimension::D2,
                &[0, 0, 0, 255],
                bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                bevy::asset::RenderAssetUsages::default(),
            );
            image.sampler = bevy::image::ImageSampler::linear();
            let image = images.add(image);
            // The set's material: Particles with SrcBlend SrcAlpha and DstBlend One, so the screen adds _Color * colour * frame.
            let Some(m) = g.renderers[r].material else { unimplemented!("mod-only: a VideoPlayer on a renderer without a material") };
            let m = crate::assets::components::Material::read(db.data(m)).unwrap_or_else(|e| panic!("the screen's material: {e}"));
            if (m.float("_SrcBlend"), m.float("_DstBlend")) != (Some(5.0), Some(1.0)) {
                unimplemented!("mod-only: a VideoPlayer on a material that is not SrcAlpha One");
            }
            let k = m.color("_Color").unwrap_or([1.0; 4]);
            let Some((entity, _, mut tint)) = sprites.iter_mut().find(|(_, s, _)| s.0 == r) else { continue };
            let Drawn::Mesh(mesh, _) = &tint.to else { unimplemented!("mod-only: a VideoPlayer on a screen that is not a plain sprite") };
            let mesh = mesh.clone();
            let handle = glows.add(crate::lights::Glow { color: Vec4::ZERO, texture: image.clone() });
            commands.entity(entity).remove::<MeshMaterial2d<ColorMaterial>>().insert(MeshMaterial2d(handle.clone()));
            tint.to = Drawn::Glow(mesh, handle, k, 1.0);
            tint.color = [f32::NAN; 4];
            screens.insert(t, VideoScreen { clip: wanted, video, image, serial: 0 });
        }
        let s = screens.get_mut(&t).expect("a screen made above");
        let Some((serial, px)) = s.video.frame_at(tv.video.time) else { continue };
        if s.serial != serial {
            s.serial = serial;
            if let Some(mut image) = images.get_mut(&s.image) {
                image.data = Some(px.to_vec());
            }
        }
    }
}

/// A NamedObject's m_Name: its first field (an aligned length-prefixed string).
pub(crate) fn object_name(db: &crate::assets::db::AssetDb, r: crate::assets::db::ObjRef) -> String {
    let d = db.data(r);
    let n = u32::from_le_bytes([d[0], d[1], d[2], d[3]]) as usize;
    String::from_utf8_lossy(&d[4..4 + n]).into_owned()
}

/// PP_RENDER rows for the physicals from `first` on and the renderers, lines and texts anchored on them.
fn render_dump(g: &crate::game::Game, first: usize, from: (usize, usize, usize)) {
    let Some(db) = g.assets.as_deref() else { return };
    let c = |c: [f32; 4]| c.map(|x| format!("{x:.2}")).join(",");
    let sprite = |s: crate::assets::db::ObjRef, on: bool, color: [f32; 4], layer: i32, order: i16| {
        println!("render	sprite	{}	{}	{}	{layer}	{order}", object_name(db, s), if on { "True" } else { "False" }, c(color));
    };
    for p in &g.physicals[first..] {
        if let Some((s, color, order)) = p.sprite {
            sprite(s, p.active && !p.isDisintegrated && p.sprite_enabled, color, p.sprite_layer, order);
        }
    }
    // CreateOutlineObject's child: the object's sprite, white, the "Top" layer (TagManager's seventh) at int.MaxValue
    // (sortingOrder reads it back as a short, -1), shown while ShowOutline is.
    for p in &g.physicals[first..] {
        if let (Some((s, _, _)), true) = (p.sprite, p.ids.outlineSpriteRenderer != 0) {
            sprite(s, p.active && p.showOutline, [1.0; 4], SORTING_LAYERS[6] as i32, -1);
        }
    }
    for r in g.renderers[from.0..].iter() {
        sprite(r.sprite.0, r.visible && r.enabled, r.sprite.1, r.layer, r.sprite.2);
    }
    for l in g.lines[from.1..].iter() {
        println!("render	line	{}	{}", if l.enabled && l.visible { "True" } else { "False" }, l.positions.len());
    }
    for w in g.world_texts[from.2..].iter() {
        println!("render	text	{}	{}", if w.visible && w.enabled && w.renderer_enabled { "True" } else { "False" }, w.text.replace('\n', "\\n"));
    }
}

/// PP_DRAWDIFF (scratch diagnostic, no behaviour change): what the item's prefab should draw (every renderer in the active
/// hierarchy) against what the rewrite's state hands Bevy (sprites that make_sprite can build, lines, texts, particle systems).
fn drawdiff(g: &mut crate::game::Game, name: &str, first: usize, from: (usize, usize, usize)) {
    use crate::assets::db::Component;
    if std::env::var("PP_MAPDUMP").is_ok() {
        if let Some(db) = g.assets.as_deref() {
            for (i, ph) in g.physicals.iter().enumerate() {
                if let Some((r, color, order)) = ph.sprite.filter(|_| !ph.destroyed && g.body_alive(i)) {
                    let c = g.world.body(ph.rigidbody.body).sweep.c;
                    println!("PHY {i} {} {} pos={:.2},{:.2} draw={:?} layer={} order={order} glayer={} color={color:?} mat={} active={} en={}", ph.name, object_name(db, r), c.x, c.y, ph.sprite_draw, ph.sprite_layer, ph.sprite_game_layer, ph.sprite_material.map_or("none".into(), |m| object_name(db, m)), ph.active, ph.sprite_enabled);
                }
            }
            for (i, r) in g.renderers.iter().enumerate() {
                let (p, q, sc) = g.anchored_world(r.anchor, &r.chain);
                let sp = db.sprite(r.sprite.0).ok();
                println!("MAP {i} {} pos={:.2},{:.2},{:.2} scale={:.3},{:.3} rotz={:.3} mode={} size={:?} flip={:?} thr={} adapt={} layer={} order={} glayer={} color={:?} mat={} vis={} en={} sprite_rect={:?} ppu={:?} border={:?} pivot={:?} bounds={:?}", object_name(db, r.sprite.0), p[0], p[1], p[2], sc[0], sc[1], q[2], r.draw.1, r.draw.2, r.draw.0, r.draw.3, r.draw.4, r.layer, r.sprite.2, r.game_layer, r.sprite.1, r.material.map_or("none".into(), |m| object_name(db, m)), r.visible, r.enabled, sp.as_ref().map(|s| s.rect), sp.as_ref().map(|s| s.pixels_to_units), sp.as_ref().map(|s| s.border), sp.as_ref().map(|s| s.pivot), r.bounds);
            }
        }
    }
    let Some(mut db) = g.assets.take() else { return };
    let mut census = std::collections::BTreeMap::<String, [usize; 2]>::new();
    match crate::game::Game::get_spawnable(&mut db, name) {
        Ok(prefab) => {
            let active_in_hierarchy = |mut o: usize| loop {
                if !prefab.objects[o].active {
                    return false;
                }
                match prefab.objects[o].parent {
                    Some(p) => o = p,
                    None => return true,
                }
            };
            for (o, obj) in prefab.objects.iter().enumerate() {
                let live = active_in_hierarchy(o);
                for (r, c) in &obj.components {
                    let (kind, on) = match c {
                        Component::SpriteRenderer(s) => ("SpriteRenderer".to_string(), s.enabled),
                        Component::Other(23) => ("MeshRenderer".into(), crate::assets::components::MeshRenderer::read(db.data(*r)).map_or(true, |m| m.enabled)),
                        Component::Other(120) => ("LineRenderer".into(), crate::assets::components::LineRendererData::read(db.data(*r)).map_or(true, |m| m.enabled)),
                        Component::Other(96) => ("TrailRenderer".into(), true),
                        Component::Other(198) => ("ParticleSystem".into(), true),
                        Component::Other(222) => ("CanvasRenderer".into(), true),
                        Component::Other(212) | Component::Other(213) => (format!("Other{}", 0), true),
                        Component::Script { class, enabled, .. } if class == "TextMeshPro" || class.contains("Light") || class.contains("Text") => (format!("Script:{class}"), *enabled),
                        _ => continue,
                    };
                    let e = census.entry(kind).or_default();
                    e[0] += 1;
                    e[1] += (on && live) as usize;
                }
            }
        }
        Err(e) => println!("DD {name} prefab-err {e}"),
    }
    let line = census.iter().map(|(k, v)| format!("{k}={}/{}", v[1], v[0])).collect::<Vec<_>>().join(" ");
    println!("DD {name} prefab(on/all): {line}");
    // The rewrite's side.
    let cache = &g.limb_sprite_cache;
    let (mut images, mut meshes, mut materials, mut glows, mut panels, mut parts) = (Assets::<Image>::default(), Assets::<Mesh>::default(), Assets::<ColorMaterial>::default(), Assets::<crate::lights::Glow>::default(), Assets::<crate::lights::Panels>::default(), Assets::<crate::particles::ParticleMaterial>::default());
    let mut textures = Textures::default();
    let mut shown = [0usize; 2];
    let mut check = |what: &str, label: &str, r, color: [f32; 4], mat, draw, on: bool| {
        let made = make_sprite(cache, &db, &mut images, &mut meshes, &mut materials, &mut glows, &mut panels, &mut parts, mat, &mut textures, r, color, draw);
        let (tag, ok) = match (&made, on) {
            (None, _) => ("NODRAW", false),
            (_, _) if color[3] <= 0.0 => ("ZEROALPHA", false),
            _ => ("", true),
        };
        if on {
            shown[0] += 1;
        }
        if ok && on {
            shown[1] += 1;
        }
        if !tag.is_empty() {
            println!("DD {name} {tag} {what} {label} on={on} color={color:?}");
        }
    };
    for p in &g.physicals[first..] {
        if let Some((r, color, _)) = p.sprite {
            if g.limbs.iter().any(|l| l.PhysicalBehaviour == g.physicals.iter().position(|q| std::ptr::eq(q, p)).unwrap()) {
                continue;
            }
            check("physical", &object_name(&db, r), r, color, p.sprite_material, p.sprite_draw, p.active && !p.isDisintegrated && p.sprite_enabled);
        }
    }
    for r in g.renderers[from.0..].iter() {
        check("renderer", &object_name(&db, r.sprite.0), r.sprite.0, r.sprite.1, r.material, r.draw, r.visible && r.enabled);
    }
    let lines = g.lines[from.1..].iter().filter(|l| l.enabled && l.visible && !l.positions.is_empty()).count();
    let texts = g.world_texts[from.2..].iter().filter(|w| w.visible && w.enabled && w.renderer_enabled).count();
    let ps = g.embedded_ps.len();
    println!(
        "DD {name} rewrite: sprites on={} drawable={} lines(on)={}/{} texts(on)={}/{} embedded_ps={} energy_blurs={}",
        shown[0], shown[1], lines, g.lines.len() - from.1, texts, g.world_texts.len() - from.2, ps, g.energy_blurs.len()
    );
    g.assets = Some(db);
    // Sprites whose world matrix is not the product of the local scales (a non-uniform parent above a rotated Transform).
    let (first, from) = if std::env::var("PP_MAPDUMP").is_ok() { (0, (0, 0, 0)) } else { (first, from) };
    for i in first..g.physicals.len() {
        if g.physicals[i].sprite.is_some() && g.physicals[i].parent.is_some() {
            let t = &g.physicals[i].transform;
            if let Some(m) = matrix_sprite_transform(&g.local_to_world(i), 0.0, 1.0) {
                if (m.scale.x - t.localScale[0].abs()).abs() > 1e-4 * m.scale.x.abs().max(1.0) || (m.scale.y - t.localScale[1].abs()).abs() > 1e-4 * m.scale.y.abs().max(1.0) {
                    println!("DD {name} XFORM physical {} local={:?} world=({:.4},{:.4})", g.physicals[i].name, t.localScale, m.scale.x, m.scale.y);
                }
            }
        }
    }
    for r in g.renderers[from.0..].iter() {
        let (_, _, s) = g.anchored_world(r.anchor, &r.chain);
        if let Some(m) = matrix_sprite_transform(&crate::unity::transform::local_to_world(&g.anchored_nodes(r.anchor, &r.chain)), 0.0, 1.0) {
            if (m.scale.x - s[0].abs()).abs() > 1e-4 * m.scale.x.abs().max(1.0) || (m.scale.y - s[1].abs()).abs() > 1e-4 * m.scale.y.abs().max(1.0) {
                println!("DD {name} XFORM renderer local={:?} world=({:.4},{:.4})", s, m.scale.x, m.scale.y);
            }
        }
    }
}
