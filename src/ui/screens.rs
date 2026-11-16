//! Whole-screen UI checks (`--ui-screens <recording>`). The recorder's PPG_UISCREENS recording holds the script of UI
//! actions, the game's config.json and ControlScheme.json and, per screen, a row for each shown Graphic and Selectable of
//! the game's canvases. This drives the same actions on the headless UI (the menu, then the map the recording loaded) and
//! compares each screen's shown text, word by word (one TMP text is a row of runs or lines here), as multisets.
//! A coverage build (`--cfg fncov`) writes each screen's counters as `$PPG_FNCOV/screen:<name>.profraw`
//! (tools/recorder/fnui_screens.py credits the functions whose code ran on a matching screen).
use super::*;
use std::collections::BTreeMap;
use std::path::Path;

/// The last "spawn"'s first selectable PhysicalBehaviour, for "select".
static SPAWNED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// TMP's rich text tags (`<b>`, `<color=#ccc>`, `</size>`...) out.
pub(super) fn untag(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        match rest[i..].find('>') {
            Some(j) if !rest[i + 1..i + j].contains('<') && j > 1 => rest = &rest[i + j + 1..],
            _ => {
                out.push('<');
                rest = &rest[i + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut c = s.chars();
    while let Some(x) = c.next() {
        if x != '\\' {
            out.push(x);
            continue;
        }
        match c.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(y) => out.push(y),
            None => {}
        }
    }
    out
}

fn base64(s: &str) -> Vec<u8> {
    let v = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        _ => 63,
    };
    let b: Vec<u8> = s.bytes().filter(|&c| c != b'=').map(v).collect();
    b.chunks(4).flat_map(|q| {
        let n = q.iter().enumerate().fold(0u32, |n, (i, &x)| n | (x as u32) << (18 - 6 * i));
        [(n >> 16) as u8, (n >> 8) as u8, n as u8].into_iter().take(q.len() - 1)
    }).collect()
}

fn words(s: &str, into: &mut BTreeMap<String, i32>, n: i32) {
    // A zero width space is as invisible as a space.
    for w in untag(s).split(|c: char| c.is_whitespace() || c == '\u{200b}').filter(|w| !w.is_empty()) {
        *into.entry(w.to_string()).or_default() += n;
    }
}

/// The game's screens: name -> shown words (TMP's UpperCase / SmallCaps (16 / 32) and LowerCase (8) styles applied).
fn game_screens(rec: &str) -> BTreeMap<String, BTreeMap<String, i32>> {
    let mut screens: BTreeMap<String, BTreeMap<String, i32>> = BTreeMap::new();
    for l in rec.lines() {
        let r: Vec<&str> = l.split('\t').collect();
        if r[0] == "uiscreen" && r.len() > 1 {
            screens.entry(r[1].to_string()).or_default();
        }
        if r[0] != "ui" || r.len() < 7 || !matches!(r[2], "tmp" | "text") {
            continue;
        }
        if r[6].split(',').nth(3).and_then(|a| a.parse::<f32>().ok()) == Some(0.0) {
            continue;
        }
        // UIStatViewBehaviour's ValueDisplay shows Steam's stats in a game run with Steam (ours: NotCompared("Steam")).
        if r[3].ends_with("/Container/Value display") {
            continue;
        }
        // TMP_Text.parseCtrlCharacters: a backslash and n or t are a line break and a tab.
        let mut s = untag(&unescape(r[4]).replace("\\n", "\n").replace("\\t", "\t"));
        let style: i32 = r.get(7).and_then(|x| x.parse().ok()).unwrap_or(0);
        if r[2] == "tmp" && style & (16 | 32) != 0 {
            s = s.to_uppercase();
        } else if r[2] == "tmp" && style & 8 != 0 {
            s = s.to_lowercase();
        }
        words(&s, screens.entry(r[1].to_string()).or_default(), 1);
    }
    screens
}

/// The words of a text, sorted, as one key.
fn key(words: &BTreeMap<String, i32>) -> String {
    words.iter().flat_map(|(w, &n)| std::iter::repeat_n(w.as_str(), n.max(0) as usize)).collect::<Vec<_>>().join(" ")
}

/// The game's buttons per screen: each shown Selectable's RectTransform rect (screen pixels from the bottom left: x0, y0,
/// x1, y1) under the words of its texts (on it or below it, inside its rect), top to bottom; one with no words is left out.
fn game_buttons(rec: &str) -> BTreeMap<String, BTreeMap<String, Vec<[f32; 4]>>> {
    let mut texts: BTreeMap<String, Vec<(String, String, [f32; 4])>> = BTreeMap::new();
    let mut buttons: BTreeMap<String, Vec<(String, [f32; 4])>> = BTreeMap::new();
    for l in rec.lines() {
        let r: Vec<&str> = l.split('\t').collect();
        if r[0] != "ui" || r.len() < 7 {
            continue;
        }
        let rect = || {
            let v: Vec<f32> = r[5].split(',').filter_map(|x| x.parse().ok()).collect();
            (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
        };
        match r[2] {
            "tmp" | "text" => {
                if let (Some(s), Some(x)) = (game_screens(l).into_values().next().filter(|w| !w.is_empty()), rect()) {
                    texts.entry(r[1].to_string()).or_default().push((unescape(r[3]), key(&s), x));
                }
            }
            // Sliders and scrollbars are not our `Press` buttons: their texts (a slider's value) key nothing.
            "selectable" | "toggle" | "input" | "dropdown" => {
                if let Some(x) = rect() {
                    buttons.entry(r[1].to_string()).or_default().push((unescape(r[3]), x));
                }
            }
            _ => {}
        }
    }
    let mut out = BTreeMap::new();
    for (screen, list) in buttons {
        let mut keyed: BTreeMap<String, Vec<[f32; 4]>> = BTreeMap::new();
        for (path, rect) in list {
            let under = format!("{path}/");
            let mine = |p: &str| p == path || p.starts_with(&under);
            let mut w = BTreeMap::new();
            let inside = |t: &[f32; 4]| {
                let (x, y) = ((t[0] + t[2]) / 2.0, (t[1] + t[3]) / 2.0);
                x >= rect[0] && x <= rect[2] && y >= rect[1] && y <= rect[3]
            };
            for (_, k, _) in texts.get(&screen).into_iter().flatten().filter(|(p, _, t)| mine(p) && inside(t)) {
                words(k, &mut w, 1);
            }
            if !w.is_empty() {
                keyed.entry(key(&w)).or_default().push(rect);
            }
        }
        out.insert(screen, keyed.into_iter().map(|(k, v)| (k, top_down(v))).collect());
    }
    out
}

/// Rects top to bottom, then left to right: the order both sides' same-named buttons pair up in.
fn top_down(mut v: Vec<[f32; 4]>) -> Vec<[f32; 4]> {
    v.sort_by(|a, b| (-a[3].round(), a[0].round()).partial_cmp(&(-b[3].round(), b[0].round())).unwrap());
    v
}

/// Our buttons (a shown `Press`) the same way: the node's laid out rect, flipped to the bottom-left origin of `height`.
/// Two method states alike: the same words, numbers within 2 (screen pixels; a scale's within 0.001 is the same word).
fn fn_state_same(game: &str, ours: &str) -> bool {
    let (g, o): (Vec<&str>, Vec<&str>) = (game.split([' ', ',']).collect(), ours.split([' ', ',']).collect());
    // Counts and other whole numbers must be equal; positions and sizes may be up to 2 apart.
    let whole = |s: &str| s.parse::<i64>().is_ok();
    g.len() == o.len() && g.iter().zip(&o).all(|(a, b)| a == b || (!(whole(a) && whole(b)) && matches!((a.parse::<f32>(), b.parse::<f32>()), (Ok(x), Ok(y)) if (x - y).abs() <= 2.0)))
}

#[test]
fn fn_state_counts_exact() {
    assert!(!fn_state_same("9 414", "9 413"));
    assert!(fn_state_same("1.27 x", "1.9 x"));
}

fn our_buttons(app: &mut App, height: f32) -> BTreeMap<String, Vec<[f32; 4]>> {
    let world = app.world_mut();
    let mut q = world.query_filtered::<Entity, With<Press>>();
    let presses: Vec<Entity> = q.iter(world).collect();
    let shown = |world: &World, mut e: Entity| loop {
        if world.get::<Node>(e).is_some_and(|n| n.display == Display::None) || world.get::<Visibility>(e) == Some(&Visibility::Hidden) || world.get::<NotCompared>(e).is_some() {
            return false;
        }
        match world.get::<ChildOf>(e) {
            Some(p) => e = p.parent(),
            None => return true,
        }
    };
    let mut keyed: BTreeMap<String, Vec<[f32; 4]>> = BTreeMap::new();
    for e in presses {
        if !shown(world, e) {
            continue;
        }
        let mut s = String::new();
        fn all(world: &World, e: Entity, s: &mut String) {
            for t in [world.get::<Text>(e).map(|t| &t.0), world.get::<TextSpan>(e).map(|t| &t.0), world.get::<ShownText>(e).map(|t| &t.0)].into_iter().flatten() {
                s.push_str(t);
                s.push(' ');
            }
            for &k in world.get::<Children>(e).into_iter().flatten() {
                all(world, k, s);
            }
        }
        all(world, e, &mut s);
        let mut w = BTreeMap::new();
        words(&s, &mut w, 1);
        let (Some(node), Some(t)) = (world.get::<ComputedNode>(e), world.get::<UiGlobalTransform>(e)) else { continue };
        if w.is_empty() {
            continue;
        }
        let (a, b) = (t.affine().transform_point2(-node.size() / 2.0), t.affine().transform_point2(node.size() / 2.0));
        keyed.entry(key(&w)).or_default().push([a.x.min(b.x), height - a.y.max(b.y), a.x.max(b.x), height - a.y.min(b.y)]);
    }
    keyed.into_iter().map(|(k, v)| (k, top_down(v))).collect()
}

/// Our shown words, from each root down: a Text with its spans, an italic run's picture (`ShownText`), a rich line's
/// runs as one text (`RichLine`); not below `Display::None`, `Visibility::Hidden` or `NotCompared` (counted by reason),
/// nor a fully transparent text.
fn our_words(app: &mut App, skipped: &mut BTreeMap<&'static str, i32>) -> BTreeMap<String, i32> {
    let world = app.world_mut();
    let mut roots = world.query_filtered::<Entity, (With<Node>, Without<ChildOf>)>();
    let roots: Vec<Entity> = roots.iter(world).collect();
    fn text_of(world: &World, e: Entity, out: &mut String) {
        if let Some(t) = world.get::<Text>(e) {
            if world.get::<TextColor>(e).is_none_or(|c| c.0.alpha() > 0.0) {
                out.push_str(&t.0);
            }
        }
        if let Some(t) = world.get::<TextSpan>(e) {
            out.push_str(&t.0);
        }
        if let Some(t) = world.get::<ShownText>(e) {
            out.push_str(&t.0);
        }
        for &k in world.get::<Children>(e).into_iter().flatten() {
            text_of(world, k, out);
        }
    }
    fn walk(world: &World, e: Entity, out: &mut BTreeMap<String, i32>, skipped: &mut BTreeMap<&'static str, i32>) {
        if world.get::<Node>(e).is_some_and(|n| n.display == Display::None) || world.get::<Visibility>(e) == Some(&Visibility::Hidden) {
            return;
        }
        if let Some(n) = world.get::<NotCompared>(e) {
            *skipped.entry(n.0).or_default() += 1;
            return;
        }
        if world.get::<RichLine>(e).is_some() || world.get::<Text>(e).is_some() || world.get::<ShownText>(e).is_some() {
            let mut s = String::new();
            text_of(world, e, &mut s);
            words(&s, out, 1);
            return;
        }
        for &k in world.get::<Children>(e).into_iter().flatten() {
            walk(world, k, out, skipped);
        }
    }
    let mut out = BTreeMap::new();
    for r in roots {
        walk(world, r, &mut out, skipped);
    }
    out
}

/// The headless menu app of `ui::tests`, with the install's assets so a map can be played.
fn app(game: &Path, cache: &Path, settings: Settings, controls: Controls, screen: [f32; 2]) -> Result<App, String> {
    scene::load(&mut crate::assets::db::AssetDb::open(game)?)?;
    let dir = std::env::temp_dir().join(format!("ppr-ui-screens-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // The game's saved contraptions, copied (they are only read; a save or delete would touch the copy): the catalog reads
    // the "Contraptions" folder of the working directory, which is the copy's.
    let from = game.join("Contraptions");
    for e in walk(&from) {
        let to = dir.join("Contraptions").join(e.strip_prefix(&from).unwrap());
        std::fs::create_dir_all(to.parent().unwrap()).map_err(|x| x.to_string())?;
        std::fs::copy(&e, &to).map_err(|x| format!("{}: {x}", e.display()))?;
    }
    // ItemPersistence: the install's unlocked items (a copy, so an Add writes only the copy).
    let p = game.join(crate::il!(str "ItemPersistence::Deserialise" 0));
    if p.exists() {
        std::fs::copy(&p, dir.join("persistence0")).map_err(|x| format!("{}: {x}", p.display()))?;
        super::data::persistence_init(&dir);
        if let Ok(n) = std::env::var("PPG_MACHINE_NAME") {
            let _ = super::data::MACHINE_NAME.set(n);
        }
    }
    super::data::WINDOWS_NEW_LINE.store(true, std::sync::atomic::Ordering::Relaxed);
    // The subscribed Workshop items the game's SteamContraptionSource lists (read only, as the rewrite's Workshop folder is).
    if let Some(w) = game.parent().and_then(|c| c.parent()).map(|s| s.join("workshop/content/1118200")) {
        let _ = crate::game::mods::WORKSHOP_DIR.set(w);
    }
    std::env::set_current_dir(&dir).map_err(|e| e.to_string())?;
    let mut app = App::new();
    // No renderer here: a render system's missing resource skips it (each error printed once) instead of panicking.
    app.set_error_handler(|e, _| {
        static SEEN: std::sync::Mutex<std::collections::BTreeSet<String>> = std::sync::Mutex::new(std::collections::BTreeSet::new());
        if SEEN.lock().unwrap().insert(e.to_string()) {
            eprintln!("headless: {e}");
        }
    });
    // One update is one 60 Hz frame of game time, as the recorder's waits count it.
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f64(1.0 / 60.0)));
    app.add_plugins((MinimalPlugins, bevy::input::InputPlugin)).init_resource::<HoverMap>().init_resource::<UiScale>();
    let mut window = Window::default();
    window.resolution.set(screen[0], screen[1]);
    app.world_mut().spawn((window, PrimaryWindow));
    // lights::setup's MainCamera without its render targets (the load resets its transform and projection).
    app.world_mut().spawn((crate::lights::MainCamera, Camera::default(), Transform::default(), crate::lights::start_projection()));
    // bevy_render's camera_system and the transform propagation, which this app lacks: the window-sized target, the
    // projection's matrix and the transform, so world_input's Single<(&Camera, &GlobalTransform)> runs.
    let size = UVec2::new(screen[0] as u32, screen[1] as u32);
    app.add_systems(PostUpdate, move |mut cams: Query<(&mut Camera, &mut Projection, &Transform, &mut GlobalTransform)>| {
        use bevy::camera::CameraProjection as _;
        for (mut cam, mut proj, t, mut g) in &mut cams {
            proj.update(size.x as f32, size.y as f32);
            cam.computed.target_info = Some(bevy::camera::RenderTargetInfo { physical_size: size, scale_factor: 1.0 });
            cam.computed.clip_from_view = proj.get_clip_from_view();
            *g = GlobalTransform::from(*t);
        }
    });
    let mut ui = new_ui(Some(game.to_path_buf()), Mode::Menu, None, None);
    ui.screen = screen;
    app.insert_resource(ui).insert_resource(settings).insert_resource(controls).insert_resource(Files { dir, settings: false, controls: false });
    add(&mut app);
    app.add_plugins(AssetPlugin { file_path: cache.to_string_lossy().into_owned(), ..default() })
        // bevy_ui's layout (and the text measuring it needs) for the buttons' rects.
        .add_plugins((bevy::text::TextPlugin, bevy::ui::UiPlugin))
        .init_asset::<Image>()
        .init_asset::<crate::limb_material::Body>()
        .init_asset::<crate::limb_material::LegacyBody>()
        .init_asset::<crate::wires::BloodWire>()
        .init_resource::<bevy::gizmos::config::GizmoConfigStore>()
        .insert_resource(crate::game_assets::HumanRig::load(cache, game)?);
    app.world_mut().resource_mut::<bevy::gizmos::config::GizmoConfigStore>().insert(default(), bevy::gizmos::config::DefaultGizmoConfigGroup);
    // No renderer: thumbnail::thumbnails never runs, so a pending SaveThumbnail picture is dropped (the picture is not
    // compared) and CatalogBehaviour.CreateItemButtons runs as after it, as Serialise does in the game.
    app.add_systems(Update, |mut play: Option<ResMut<crate::play::Play>>| {
        if let Some(c) = play.as_deref_mut().map(|p| &mut p.game.contraption) {
            if !c.thumbnails.is_empty() {
                c.thumbnails.clear();
                c.catalog_dirty = true;
            }
        }
    });
    Ok(app)
}

pub(super) fn page(name: &str) -> Option<Page> {
    Some(match name {
        "Main" => Page::Main,
        "Map Selection" => Page::Maps,
        "Settings" => Page::Settings,
        "Controls" => Page::Controls,
        "Information" => Page::Info,
        "Open-source licenses" => Page::Licenses,
        "Mods" => Page::Mods,
        "Stats" => Page::Stats,
        _ => return None,
    })
}

/// MenuController's page name for a Page (the inverse of `page`).
pub(super) fn page_name(p: Page) -> &'static str {
    match p {
        Page::Main => "Main",
        Page::Maps => "Map Selection",
        Page::Settings => "Settings",
        Page::Controls => "Controls",
        Page::Info => "Information",
        Page::Licenses => "Open-source licenses",
        Page::Mods => "Mods",
        Page::Stats => "Stats",
        // Rewrite feature: not one of the game's pages.
        Page::Achievements => "Achievements",
    }
}

/// The recorder's UiScreens actions as Acts (None: one we cannot do).
fn act(app: &mut App, verb: &str, arg: Option<&str>) -> Result<Vec<Act>, String> {
    let n = || arg.and_then(|a| a.parse::<usize>().ok()).ok_or(format!("{verb} needs a number"));
    Ok(match verb {
        "page" => vec![Act::Nav(arg.and_then(page).ok_or(format!("no page {arg:?}"))?)],
        "tab" => vec![Act::Tab(n()?)],
        "selectmap" => {
            let ui = app.world().resource::<Ui>();
            vec![Act::SelectMap(ui.maps.iter().position(|m| Some(m.name.as_str()) == arg).ok_or(format!("no map {arg:?}"))?)]
        }
        // Global.SetPausedMenu(true, page): the pause menu on that page.
        "pause" => {
            let mut v = if app.world().resource::<Ui>().mode == Mode::Pause { vec![] } else { vec![Act::Escape] };
            v.push(Act::Nav(page(arg.unwrap_or("Main")).ok_or(format!("no page {arg:?}"))?));
            v
        }
        "resume" => vec![Act::Resume],
        "category" => vec![Act::Category(n()?)],
        // Global.ToggleSlowmotion, TogglePaused and ToggleLimbStatus.
        "slowmo" => vec![Act::Slowmo],
        "timetoggle" => vec![Act::TimeToggle],
        "limbstatus" => vec![Act::LimbStatus],
        // CatalogBehaviour.SetItem(GetSpawnable(arg)).
        "item" => vec![Act::Item(arg.ok_or("item needs a name")?.to_string())],
        // ToolControllerBehaviour.SetToolByTypeName(arg): the toolbar button of that type.
        "tool" => {
            let world = app.world_mut();
            let found = world.query::<&Press>().iter(world).find_map(|p| match &p.0 {
                Act::Tool(name, ty) if Some(ty.as_str()) == arg => Some(Act::Tool(name.clone(), ty.clone())),
                _ => None,
            });
            vec![found.ok_or(format!("no tool {arg:?}"))?]
        }
        // ToolWheelBehaviour.SetTab(arg) (0 tools, 1 powers) and ToggleToolVisibility.
        "toolstab" => vec![Act::Powers(n()? == 1)],
        "toolvis" => vec![Act::ToolVisibility],
        // The spawnable `arg` at the camera (as the recorder instantiates it); "select" (a frame later, once
        // its PhysicalBehaviours started and joined PhysicalObjectsInWorld) selects its first PhysicalBehaviour alone
        // (GetComponentInChildren<PhysicalBehaviour>).
        "select" | "seladd" => {
            let mut play = app.world_mut().resource_mut::<crate::play::Play>();
            let p = SPAWNED.load(std::sync::atomic::Ordering::Relaxed);
            play.game.select_for_test(p, verb == "seladd");
            vec![]
        }
        "spawn" => {
            let mut play = app.world_mut().resource_mut::<crate::play::Play>();
            let g = &mut play.game;
            let first = g.physicals.len();
            let mut db = g.assets.take().ok_or("no game data")?;
            // At the camera (the world point of the screen's centre), as the recorder does.
            let c = match (g.input.screen, g.input.window) {
                (Some(s), Some(w)) => s.to_world([w.size[0] / 2.0, w.size[1] / 2.0]),
                _ => [g.BoundingBox.center[0], g.BoundingBox.center[1]],
            };
            let spawned = g.catalog_spawn(&mut db, arg.ok_or("spawn needs a name")?, [c[0], c[1], 0.0], false);
            g.assets = Some(db);
            g.unsynced.push(spawned?.0);
            let p = (first..g.physicals.len()).find(|&q| g.physicals[q].Selectable).ok_or("nothing selectable spawned")?;
            SPAWNED.store(p, std::sync::atomic::Ordering::Relaxed);
            vec![]
        }
        // ContextMenuBehaviour.Show at the screen's centre (its dynamic buttons from the selection), and Hide.
        "context" => {
            let mut play = app.world_mut().resource_mut::<crate::play::Play>();
            play.game.context_menu_show();
            let buttons = play.game.context_menu_buttons();
            let mut ui = app.world_mut().resource_mut::<Ui>();
            let screen = ui.screen;
            super::show_context(&mut ui, [screen[0] / 2.0, screen[1] / 2.0], screen, buttons);
            vec![]
        }
        // The spawned item's selection flipped, deleted by the user, cleared or deselected (SelectionController).
        // CatalogBehaviour.Update's spawn key release (HideContraptionOutline, then Spawn of the selected contraption or item; "left" flipped),
        // ShowContraptionOutline and HideContraptionOutline, and get_SelectedCategory.
        "catspawn" => {
            let key = if arg == Some("left") { "spawnLeft" } else { "spawnRight" };
            app.world_mut().resource_mut::<Ui>().hide_contraption_outline();
            let mut play = app.world_mut().resource_mut::<crate::play::Play>();
            let g = &mut play.game;
            g.input.controls.insert(key.into(), crate::game::tool::Control { down: false, held: false, up: true });
            g.catalog_update();
            g.input.controls.insert(key.into(), Default::default());
            vec![]
        }
        "catoutline" => {
            app.world_mut().resource_mut::<Ui>().show_contraption_outline(arg == Some("left"));
            vec![]
        }
        "catunoutline" => {
            app.world_mut().resource_mut::<Ui>().hide_contraption_outline();
            vec![]
        }
        "selcat" => {
            app.world().resource::<Ui>().selected_category();
            vec![]
        }
        "selflip" | "seldelete" | "selclear" | "deselect" => {
            let mut play = app.world_mut().resource_mut::<crate::play::Play>();
            let g = &mut play.game;
            match verb {
                "selflip" => g.flip_selection(),
                "seldelete" => g.user_delete_selection(),
                "selclear" => g.clear_selection(),
                _ => g.deselect(SPAWNED.load(std::sync::atomic::Ordering::Relaxed)),
            }
            vec![]
        }
        // The open entry box's InputField.text set: unlike typing, the setter does not validate (uisbb2: "abc" stays in a
        // DecimalNumber field).
        "type" => {
            let mut play = app.world_mut().resource_mut::<crate::play::Play>();
            let e = play.game.entry.as_mut().ok_or("no entry box")?;
            let t = arg.unwrap_or("").to_string();
            e.text = t.clone();
            // The shown input field is the Ui's copy (the one Apply hands back to the game).
            let mut ui = app.world_mut().resource_mut::<Ui>();
            if let Some(u) = ui.entry.as_mut() {
                u.text = t;
            }
            ui.entry_all = false;
            ui.dirty = true;
            vec![]
        }
        // A static context menu button (its GameObject name) clicked.
        "cmbutton" => {
            let k = super::tables::CONTEXT_MENU.iter().position(|b| Some(b.0) == arg).ok_or(format!("no context button {arg:?}"))?;
            vec![Act::MenuStatic(k)]
        }
        "uncontext" => {
            super::hide_context(&mut app.world_mut().resource_mut::<Ui>());
            vec![]
        }
        // The saved contraption named `arg`: its catalog button pressed, or its info dialog.
        "contraption" | "iteminfo" => {
            let ui = app.world().resource::<Ui>();
            let i = ui.catalog.items.iter().position(|it| it.local.as_ref().is_some_and(|m| Some(m.name()) == arg)).ok_or(format!("no contraption {arg:?}"))?;
            vec![if verb == "contraption" { Act::Contraption(i) } else { Act::ItemInfo(i) }]
        }
        // TriggerEditorBehaviour.StartWaiting and Unbind of the action `arg`.
        "rebind" | "unbind" => {
            let i = super::tables::ACTIONS.iter().position(|a| Some(a.id) == arg).ok_or(format!("no action {arg:?}"))?;
            vec![if verb == "rebind" { Act::Rebind(i) } else { Act::Unbind(i) }]
        }
        // The key `arg` (a KeyCode name) down for a frame and up the next, as the window would send them.
        "key" => {
            let Some(keys::Src::Key(key_code)) = arg.and_then(keys::src) else { return Err(format!("no key {arg:?}")) };
            let window = app.world_mut().query_filtered::<Entity, With<PrimaryWindow>>().single(app.world()).map_err(|e| e.to_string())?;
            for state in [bevy::input::ButtonState::Pressed, bevy::input::ButtonState::Released] {
                app.world_mut().write_message(bevy::input::keyboard::KeyboardInput { key_code, logical_key: bevy::input::keyboard::Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified), state, text: None, repeat: false, window });
                app.update();
            }
            vec![]
        }
        // The shown text `arg`'s button.
        // clicktrim: the text without its trailing whitespace (the colour picker's Apply with its line break).
        "click" | "clicktrim" => {
            let world = app.world_mut();
            let hits: Vec<Entity> = world.query::<(Entity, &Text)>().iter(world).filter(|(_, t)| Some(if verb == "clicktrim" { t.0.trim_end() } else { t.0.as_str() }) == arg).map(|(e, _)| e).collect();
            let mut q = world.query::<(Option<&Press>, Option<&ChildOf>)>();
            let mut found = None;
            'hits: for mut e in hits {
                while let Ok((press, parent)) = q.get(world, e) {
                    if let Some(p) = press {
                        found = Some(p.0.clone());
                        break 'hits;
                    }
                    let Some(p) = parent else { break };
                    e = p.parent();
                }
            }
            vec![found.ok_or(format!("no button {arg:?}"))?]
        }
        // The pointer entering / leaving the shown text `arg`: Hovered on it and its parents (no picking backend here sets it).
        "hover" | "unhover" => {
            let world = app.world_mut();
            let hits: Vec<Entity> = world.query::<(Entity, &Text)>().iter(world).filter(|(_, t)| Some(t.0.as_str()) == arg).map(|(e, _)| e).collect();
            let hoverable = |mut e: Entity| loop {
                if world.get::<Hovered>(e).is_some() {
                    return true;
                }
                match world.get::<ChildOf>(e) {
                    Some(p) => e = p.parent(),
                    None => return false,
                }
            };
            let hit = hits.iter().copied().find(|&e| hoverable(e)).ok_or(format!("no hoverable text {arg:?}"))?;
            let mut e = Some(hit);
            while let Some(x) = e {
                if world.get::<Hovered>(x).is_some() {
                    world.entity_mut(x).insert(Hovered(verb == "hover"));
                }
                e = world.get::<ChildOf>(x).map(|p| p.parent());
            }
            vec![]
        }
        // DialogBoxManager.Notification(arg) and NotificationControllerBehaviour.Show(arg).
        "notify" => {
            let mut ui = app.world_mut().resource_mut::<Ui>();
            ui.open_dialog(state::DialogView::notification(arg.unwrap_or_default()));
            ui.dirty = true;
            vec![]
        }
        "toast" => {
            let now = app.world().resource::<Time>().elapsed_secs_f64();
            let mut ui = app.world_mut().resource_mut::<Ui>();
            ui.notify(arg.unwrap_or_default(), now);
            ui.dirty = true;
            vec![]
        }
        _ => return Err(format!("unknown action {verb}")),
    })
}

/// The recorder's UiNotes mark `at` ("menu" before the menu's screens, "loaded" before the map's): our notifications
/// get the game's ages there (the load before took real time we do not share); returns the time the screens count from.
fn sync_notes(app: &mut App, rec: &str, at: &str) -> f64 {
    let now = app.world().resource::<Time<Real>>().elapsed_secs_f64();
    let mut ui = app.world_mut().resource_mut::<Ui>();
    for l in rec.lines() {
        if let ["uinote", a, text, age] = l.split('\t').collect::<Vec<_>>().as_slice() {
            let (text, age) = (unescape(text), age.parse::<f64>().unwrap_or(0.0));
            if *a == at {
                if let Some(n) = ui.notes.iter_mut().find(|(t, _)| untag(t) == untag(&text)) {
                    n.1 = now - age;
                }
            }
        }
    }
    ui.dirty = true;
    now
}

/// The files under `dir`, recursively (none when it is missing).
fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().flat_map(|e| if e.path().is_dir() { walk(&e.path()) } else { vec![e.path()] }).collect()
}

pub fn run(recording: &Path, game: &Path, cache: &Path) -> Result<(), String> {
    let rec = std::fs::read_to_string(recording).map_err(|e| format!("{}: {e}", recording.display()))?;
    let mut settings = Settings::default();
    let mut controls = Controls::default();
    let mut script = Vec::new();
    let mut screen = [1920.0, 1080.0];
    let mut map = "Default".to_string();
    // Each screen's dump time after its part's start (unscaled seconds), when recorded.
    let mut at: BTreeMap<String, f64> = BTreeMap::new();
    for l in rec.lines() {
        let r: Vec<&str> = l.split('\t').collect();
        match r.as_slice() {
            ["uiscript", rest @ ..] => script.push(rest.to_vec()),
            // SteamContraptionSource's cache (Steam's data): "<IsInitialised>" then "<id>=<Title>" per item.
            ["uisteam", _, items @ ..] => {
                let _ = crate::game::mods::STEAM_CACHE.set(items.iter().filter_map(|i| i.split_once('=')).filter_map(|(id, t)| Some((id.parse().ok()?, t.to_string()))).collect());
            }
            ["uisteam"] => {}
            ["uifile", "config.json", b] if *b != "-" => settings.load(&String::from_utf8_lossy(&base64(b))),
            ["uifile", "ControlScheme.json", b] if *b != "-" => controls.load(&String::from_utf8_lossy(&base64(b))),
            ["uiscreen", name, size, ..] => {
                if let Some(t) = r.get(4).and_then(|t| t.parse().ok()) {
                    at.insert(name.to_string(), t);
                }
                if let Some((w, h)) = size.split_once('x') {
                    screen = [w.parse().unwrap_or(1920.0), h.parse().unwrap_or(1080.0)];
                }
            }
            _ if l.starts_with("map ") => map = l[4..].to_string(),
            _ => {}
        }
    }
    let game_words = game_screens(&rec);
    let game_rects = game_buttons(&rec);
    // uifn <screen> <method> <state>: the per-function checks' game side.
    let mut game_fns: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for l in rec.lines().filter(|l| l.starts_with("uifn\t")) {
        let c: Vec<&str> = l.split('\t').collect();
        if c.len() == 4 {
            game_fns.entry(c[1].into()).or_default().insert(c[2].into(), c[3].into());
        }
    }
    let mut started = false;
    super::UIFN_ON.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut app = app(game, cache, settings, controls, screen)?;
    // The menu's launch: the maps registry filled (MapLoadStep).
    for _ in 0..3000 {
        app.update();
        if !app.world().resource::<Ui>().maps.is_empty() {
            break;
        }
    }
    for _ in 0..30 {
        app.update();
    }
    let mut base = sync_notes(&mut app, &rec, "menu");
    let (mut same, mut total) = (0, 0);
    // The first screen's counters run from the process's start (the menu's launch built it), the first after "load"
    // from the load's start (the map's launch built the HUD); the others from their own actions.
    let mut fresh = true;
    for line in &script {
        let name = line[0];
        if name == "load" {
            #[cfg(fncov)]
            crate::game::dump::fncheck::cov::start();
            fresh = true;
            let i = app.world().resource::<Ui>().maps.iter().position(|m| m.name == map).ok_or(format!("no map {map}"))?;
            app.world_mut().resource_mut::<Queue>().0.extend([Act::Nav(Page::Main), Act::SelectMap(i), Act::Enter]);
            for _ in 0..3000 {
                app.update();
                if app.world().resource::<Ui>().mode == Mode::Game && app.world().get_resource::<Play>().is_some() {
                    break;
                }
            }
            if app.world().resource::<Ui>().mode != Mode::Game {
                let ui = app.world().resource::<Ui>();
                return Err(format!("{map} did not load: mode {:?} error {:?} play {}", ui.mode, ui.error, app.world().get_resource::<Play>().is_some()));
            }
            for _ in 0..30 {
                app.update();
            }
            base = sync_notes(&mut app, &rec, "loaded");
            continue;
        }
        if !fresh {
            #[cfg(fncov)]
            crate::game::dump::fncheck::cov::start();
        }
        fresh = false;
        for a in line.get(1).map(|s| s.split(';').filter(|s| !s.is_empty()).collect::<Vec<_>>()).unwrap_or_default() {
            let (verb, arg) = match a.split_once(':') {
                Some((v, x)) => (v, Some(x)),
                None => (a, None),
            };
            match act(&mut app, verb, arg) {
                Ok(acts) => app.world_mut().resource_mut::<Queue>().0.extend(acts),
                Err(e) => println!("uiact\t{name}\t{a}\t!{e}"),
            }
            app.update();
        }
        // The recorder's 1 s at 60 frames a second, or on to the game's dump time.
        for _ in 0..60 {
            app.update();
        }
        if let Some(t) = at.get(name) {
            while app.world().resource::<Time<Real>>().elapsed_secs_f64() - base < *t {
                app.update();
            }
        }
        let mut skipped = BTreeMap::new();
        let ours = our_words(&mut app, &mut skipped);
        // The buttons both sides name the same way: their rects within 2 px.
        let our_rects = our_buttons(&mut app, screen[1]);
        let (mut matched, mut off) = (0, Vec::new());
        // Same-named buttons pair up top to bottom; a name with a different count on each side is listed.
        for (k, gs) in game_rects.get(name).into_iter().flatten() {
            let Some(os) = our_rects.get(k) else { continue };
            if gs.len() != os.len() {
                off.push(format!("[{k}] {} in the game, {} ours", gs.len(), os.len()));
                continue;
            }
            for (g, o) in gs.iter().zip(os) {
                matched += 1;
                if g.iter().zip(o).any(|(a, b)| (a - b).abs() > 2.0) {
                    off.push(format!("[{k}] game {:.0},{:.0},{:.0},{:.0} ours {:.0},{:.0},{:.0},{:.0}", g[0], g[1], g[2], g[3], o[0], o[1], o[2], o[3]));
                }
            }
        }
        if off.is_empty() {
            println!("layout\t{name}\tsame\t{matched}");
        } else {
            println!("layout\t{name}\tDIFF\t{} of {matched}\t{}", off.len(), off.join("\t"));
        }
        if !skipped.is_empty() {
            println!("skipped\t{name}\t{skipped:?}");
        }
        // The per-function checks: each method's state on this screen, the game's against ours.
        let mut ui = app.world_mut().resource_mut::<Ui>();
        let mut our_fns = std::mem::take(&mut *super::UIFN.lock().unwrap());
        if !started && game_fns.get(name).is_some_and(|f| f.contains_key("ContextMenuBehaviour.Start")) {
            // ContextMenuBehaviour.Start: the menu's state before any act opened it.
            started = true;
            our_fns.insert("ContextMenuBehaviour.Start", super::cs(ui.context.is_some()).to_string());
        }
        // NotificationAnimationBehaviour.Update: its t is the screen's timing (the harness's frames are not the game's) and the
        // last of several notifications wins, so ours is our offset for the game's t and renderedWidth.
        if let Some([t, w]) = game_fns.get(name).and_then(|f| f.get("NotificationAnimationBehaviour.Update")).and_then(|g| {
            let v: Vec<&str> = g.split(' ').collect();
            Some([v.first()?.parse::<f64>().ok()?, v.get(1)?.parse::<f64>().ok()?])
        }) {
            let o = super::note_offset(&super::note_offset_curve(), &mut Default::default(), t, w as f32);
            our_fns.insert("NotificationAnimationBehaviour.Update", format!("{t} {w} {o}"));
        }
        for (f, g) in game_fns.get(name).into_iter().flatten() {
            let o = our_fns.get(f.as_str()).cloned().unwrap_or_else(|| "none".into());
            println!("uifn\t{name}\t{f}\t{}\tgame {g} ours {o}", if fn_state_same(g, &o) { "same" } else { "DIFF" });
        }
        #[cfg(fncov)]
        {
            crate::game::dump::fncheck::cov::stash();
            crate::game::dump::fncheck::cov::passed(&format!("screen:{name}"));
        }
        total += 1;
        let Some(theirs) = game_words.get(name) else {
            println!("screen\t{name}\tNOGAME");
            continue;
        };
        let mut diff: BTreeMap<String, i32> = theirs.clone();
        for (w, n) in &ours {
            *diff.entry(w.clone()).or_default() -= n;
        }
        diff.retain(|_, n| *n != 0);
        if diff.is_empty() {
            same += 1;
            println!("screen\t{name}\tsame\t{}", ours.values().sum::<i32>());
        } else {
            let missing: Vec<String> = diff.iter().filter(|(_, n)| **n > 0).map(|(w, n)| format!("{w}x{n}")).collect();
            let extra: Vec<String> = diff.iter().filter(|(_, n)| **n < 0).map(|(w, n)| format!("{w}x{}", -n)).collect();
            println!("screen\t{name}\tDIFF\tmissing {}\textra {}", missing.join(" "), extra.join(" "));
        }
    }
    #[cfg(fncov)]
    crate::game::dump::fncheck::cov::write();
    println!("screens {same} same of {total}");
    // The scratch folder (the contraptions' copy, the settings written) goes.
    let scratch = std::env::current_dir().map_err(|e| e.to_string())?;
    std::env::set_current_dir(std::env::temp_dir()).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(scratch);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn untag_and_base64() {
        assert_eq!(super::untag("<color=#ccc>Loading</color> a<b"), "Loading a<b");
        assert_eq!(super::base64("eyJhIjoxfQ=="), b"{\"a\":1}");
    }
}
