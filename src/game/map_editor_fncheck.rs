//! --fn-check of a recorder PPG_MAPEDITOR recording (tools/recorder/MapEditorProbe.cs): each case's editor state before
//! ("G"/"S" lines) becomes a `MapEditor`, the case's function runs on it, and its return and the state after are compared
//! with the game's ("g"/"s" lines, "R"). Floats are bit-exact (8 hex digits).
use super::*;
use std::collections::BTreeMap;

fn f(h: &str) -> f32 {
    f32::from_bits(u32::from_str_radix(h, 16).unwrap_or_else(|_| panic!("bad float {h}")))
}
fn h(v: f32) -> String {
    format!("{:08x}", v.to_bits())
}
fn fs(s: &str) -> Vec<f32> {
    s.split(',').map(f).collect()
}
fn ids(s: &str) -> Vec<usize> {
    s.split(';').filter(|x| !x.is_empty()).map(|x| x.parse().unwrap()).collect()
}
fn ids_text(v: &[usize]) -> String {
    v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(";")
}
fn b(s: &str) -> bool {
    s == "1"
}
fn bt(v: bool) -> &'static str {
    if v { "1" } else { "0" }
}
/// -0 and 0 compare equal (the engine's Transform getters return either for an identity rotation's zeros).
fn canon(s: &str) -> String {
    s.split_inclusive(&[',', '|', ' '][..]).map(|t| t.replacen("80000000", "00000000", if t.trim_end_matches(&[',', '|', ' '][..]) == "80000000" { 1 } else { 0 })).collect()
}

/// The differing elements of two comma/pipe lists: "[index] game/ours".
fn elems(g: &str, o: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (g.split(&[',', '|'][..]).collect(), o.split(&[',', '|'][..]).collect());
    if a.len() != b.len() {
        return format!("game {g} ours {o}");
    }
    a.iter().zip(&b).enumerate().filter(|(_, (x, y))| x != y).map(|(i, (x, y))| format!("[{i}] {x}/{y}")).collect::<Vec<_>>().join(" ")
}

/// The move gizmo's three handles are its children: their world pose is the engine hierarchy's (the rewrite keeps
/// only the gizmo's own pose and each handle's IsBeingDragged), so only the drag flag of those three is compared.
fn move_handles_drag_only(h: &str) -> String {
    h.split('|').enumerate().map(|(i, x)| if i < 3 { x.split(',').next().unwrap_or("").to_string() } else { x.to_string() }).collect::<Vec<_>>().join("|")
}

fn kv(line: &str) -> BTreeMap<&str, &str> {
    line.split(' ').filter_map(|t| t.split_once('=')).collect()
}

struct Sel {
    live: bool,
    depth: i32,
    age: i32,
    pos: [f32; 3],
    rot: [f32; 4],
    scale: [f32; 3],
    lb: Vec<f32>,
    obj: bool,
    cols: Option<Vec<Shape>>,
    /// The parent's localPosition and the object's localPosition (the recorded chain: one parent that only translates).
    parent: [f32; 3],
    local: [f32; 3],
}

fn parse_sel(line: &str) -> Sel {
    let m = kv(line);
    if line.ends_with(" dead") {
        return Sel { live: false, depth: 0, age: 0, pos: [0.0; 3], rot: IDENTITY, scale: [1.0; 3], lb: vec![], obj: false, cols: None, parent: [0.0; 3], local: [0.0; 3] };
    }
    let p = fs(m["pos"]);
    let r = fs(m["rot"]);
    let s = fs(m["scale"]);
    let cols = m["cols"].split('|').filter(|c| !c.is_empty()).map(|c| {
        let (k, v) = c.split_once(':')?;
        Some(match k {
            "B" => { let x = fs(v); Shape::Box { center: [x[0], x[1]], size: [x[2], x[3]] } }
            "C" => { let x = fs(v); Shape::Circle { center: [x[0], x[1]], radius: x[2] } }
            "P" => Shape::Polygon(v.split(';').map(|q| { let x = fs(q); [x[0], x[1]] }).collect()),
            _ => return None,
        })
    }).collect::<Option<Vec<_>>>();
    let (parent, local) = match m.get("par").filter(|x| !x.is_empty()) {
        None => ([0.0; 3], [p[0], p[1], p[2]]),
        Some(c) => {
            let x = fs(c);
            assert!(!c.contains('/') && x[3..10] == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], "selectable parent chain the checker doesn't model: {c}");
            let l = fs(m["lpos"]);
            ([x[0], x[1], x[2]], [l[0], l[1], l[2]])
        }
    };
    Sel { parent, local, live: true, depth: m["depth"].parse().unwrap(), age: m["age"].parse().unwrap(), pos: [p[0], p[1], p[2]], rot: [r[0], r[1], r[2], r[3]], scale: [s[0], s[1], s[2]], lb: fs(m["lb"]), obj: b(m["obj"]), cols }
}

struct Case<'a> {
    k: usize,
    name: &'a str,
    this: usize,
    screen: [f32; 2],
    pre_g: &'a str,
    pre_order: Vec<usize>,
    pre_s: Vec<Sel>,
    sorted: Option<Vec<usize>>,
    post_g: &'a str,
    post_order: Vec<usize>,
    post_s: Vec<Sel>,
    ret: &'a str,
}

fn order_of(line: &str) -> Vec<usize> {
    ids(line.split_once("order=").map_or("", |x| x.1))
}

fn cases(text: &str) -> Vec<Case<'_>> {
    let mut out = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(l) = lines.next() {
        let Some(rest) = l.strip_prefix("C ") else { continue };
        let mut w = rest.split(' ');
        let k = w.next().unwrap().parse().unwrap();
        let name = w.next().unwrap();
        let m = kv(rest);
        let sc = if m["screen"] == "0,0" { vec![0.0, 0.0] } else { fs(m["screen"]) };
        let pre_g = lines.next().unwrap();
        let pre_order = order_of(lines.next().unwrap());
        let mut pre_s = Vec::new();
        while lines.peek().is_some_and(|x| x.starts_with("S ")) {
            pre_s.push(parse_sel(lines.next().unwrap()));
        }
        let sorted = lines.next_if(|x| x.starts_with("sorted")).map(|x| ids(x.strip_prefix("sorted").unwrap().trim()));
        let post_g = lines.next().unwrap();
        let post_order = order_of(lines.next().unwrap());
        let mut post_s = Vec::new();
        while lines.peek().is_some_and(|x| x.starts_with("s ")) {
            post_s.push(parse_sel(lines.next().unwrap()));
        }
        let ret = lines.next().unwrap().strip_prefix("R ").unwrap();
        out.push(Case { k, name, this: m["self"].parse().unwrap_or(usize::MAX), screen: [sc[0], sc[1]], pre_g, pre_order, pre_s, sorted, post_g, post_order, post_s, ret });
    }
    out
}

fn handle(v: &[f32], drag: bool) -> Handle {
    Handle { dragged: drag, position: [v[0], v[1], v[2]], rotation: [v[3], v[4], v[5], v[6]], scale: [v[7], v[8], v[9]] }
}

/// The case's editor: every selectable added in index order, the dead ones destroyed in `deaths` order (the HashSet's
/// order then matches), then the recorded fields.
fn build(c: &Case, deaths: &[usize], lb_from_record: bool) -> Result<(MapEditor, Input), String> {
    let g = kv(c.pre_g);
    let cam = fs(&g["cam"].split(',').take(8).collect::<Vec<_>>().join(","));
    let cw: Vec<&str> = g["cam"].split(',').collect();
    if cam[3..7] != [-0.0f32, -0.0, -0.0, 1.0][..] && cam[3..7] != [0.0f32, 0.0, 0.0, 1.0][..] {
        return Err("camera turned".into());
    }
    if cw[10] != "1" {
        return Err("perspective camera".into());
    }
    let mut e = MapEditor::new(EditorCamera { position: [cam[0], cam[1], cam[2]], orthographicSize: f(cw[7]), pixelWidth: cw[8].parse::<f32>().unwrap(), pixelHeight: cw[9].parse::<f32>().unwrap() });
    for (i, s) in c.pre_s.iter().enumerate() {
        let cols = match (&s.cols, deaths.contains(&i)) {
            (Some(cols), _) => cols.clone(),
            (None, true) => vec![],
            (None, false) => return Err(format!("selectable {i}: a collider shape the checker cannot load")),
        };
        let id = e.add_selectable(Transform { position: s.pos, parent: s.parent, local: s.local, rotation: s.rot, scale: s.scale }, s.depth, cols, s.obj.then(ObjectBehaviour::default));
        assert_eq!(id, i);
    }
    for &d in deaths {
        e.object_delete(d);
        e.flush_destroyed();
    }
    for (i, s) in c.pre_s.iter().enumerate() {
        if !s.live {
            continue;
        }
        let x = &mut e.selectables[i];
        x.selectionAge = s.age;
        if lb_from_record {
            x.LocalBounds = Bounds { center: [s.lb[0], s.lb[1]], extents: [s.lb[3], s.lb[4]] };
        }
    }
    e.WorldMousePos = { let v = fs(g["wmp"]); [v[0], v[1]] };
    e.manipulator.previousMousePos = { let v = fs(g["prev"]); [v[0], v[1]] };
    let cat: Vec<&str> = g["cat"].split(',').collect();
    e.ui.catalog_active = b(cat[0]);
    e.ui.catalog_width = f(cat[1]);
    let ui: Vec<&str> = g["ui"].split(',').collect();
    e.ui.pointer_over_ui = b(ui[0]);
    e.ui.dialog_open = b(ui[1]);
    e.ui.mouse_over_emitter = b(ui[2]);
    e.ui.selected_in_context_menu = b(ui[3]);
    e.ui.mouse = [f(ui[4]), f(ui[5])];
    e.ui.screen = [ui[7].parse().unwrap(), ui[8].parse().unwrap()];
    e.uiLockingRegistrations = g["locks"].parse().unwrap();
    // The lifecycle cases' fields (recordings from round 14 on).
    if let Some(u) = g.get("uni") {
        e.CurrentUniverse = [Universe::None, Universe::All, Universe::Game, Universe::MapEditor][u.parse::<usize>().unwrap()];
        let p: Vec<&str> = g["pause"].split(',').collect();
        (e.PauseMenu, e.Catalog) = (b(p[0]), b(p[1]));
        let l: Vec<u32> = g["lis"].split(',').map(|x| x.parse().unwrap()).collect();
        e.listeners = [l[0], l[1]];
        let m: Vec<i32> = g["mb"].split(',').map(|x| x.parse().unwrap()).collect();
        e.MapProperties.MapBounds = view::MapBounds { BoundsCenterInMeters: [m[0], m[1]], BoundsSizeInMeters: [m[2], m[3]] };
    }
    for (name, gz) in [("move", &mut e.manipulator.MoveGizmo), ("resize", &mut e.manipulator.ResizeGizmo)] {
        let v: Vec<&str> = g[name].split(',').collect();
        let x: Vec<f32> = v[1..].iter().map(|t| f(t)).collect();
        *gz = Gizmo { active: b(v[0]), position: [x[0], x[1], x[2]], rotation: [x[3], x[4], x[5], x[6]] };
    }
    let hs: Vec<Handle> = g["h"].split('|').map(|t| { let v: Vec<&str> = t.split(',').collect(); handle(&v[1..].iter().map(|x| f(x)).collect::<Vec<_>>(), b(v[0])) }).collect();
    let m = &mut e.manipulator;
    [m.AllAxisMoveGizmo, m.VerticalMoveGizmo, m.HorizontalMoveGizmo, m.TopResizeHandle, m.RightResizeHandle, m.LeftResizeHandle, m.BottomResizeHandle] = [hs[0], hs[1], hs[2], hs[3], hs[4], hs[5], hs[6]];
    m.CenterOfSelection = { let v = fs(g["center"]); [v[0], v[1]] };
    e.CurrentlySelected = ids(g["sel"]);
    e.HoveringOver = ids(g["hov"]);
    // menu=shown,pos(3),sizeDelta(2),referenceResolution(2),match,delete,copy,paste,selectablesWhenShown,selectedObjects
    let mv: Vec<&str> = g["menu"].split(',').collect();
    let cm = &mut e.context_menu;
    cm.shown = b(mv[0]);
    cm.position = [f(mv[1]), f(mv[2]), f(mv[3])];
    // Show's ForceRebuildLayoutImmediate: the size it rebuilt to (the case's after state).
    let size_from = if c.name == "MapEditorContextMenuBehaviour.Show" { kv(c.post_g)["menu"].split(',').map(String::from).collect::<Vec<_>>() } else { mv.iter().map(|x| x.to_string()).collect() };
    cm.size_delta = [f(&size_from[4]), f(&size_from[5])];
    cm.reference_resolution = [f(mv[6]), f(mv[7])];
    cm.match_width_or_height = f(mv[8]);
    (cm.DeleteInteractable, cm.CopyInteractable, cm.PasteInteractable) = (b(mv[9]), b(mv[10]), b(mv[11]));
    // A destroyed selectable the menu still holds is recorded as -1 (no index): it is gone, and destroying it again is a no-op.
    let live_ids = |s: &str| s.split(';').filter_map(|x| x.parse::<usize>().ok()).collect::<Vec<_>>();
    cm.selectablesWhenShown = live_ids(mv[12]);
    cm.selectedObjects = live_ids(mv[13]);
    let mut input = Input::default();
    for key in g["keys"].split(';').filter(|k| !k.is_empty()) {
        let (n, k) = key.split_once(':').unwrap();
        let c = input.controls.entry(n.to_string()).or_default();
        match k {
            "d" => c.down = true,
            "h" => c.held = true,
            _ => c.up = true,
        }
    }
    Ok((e, input))
}

/// The state the recorder writes, of the fields this checker compares (the inputs it only reads are left out).
fn state(e: &MapEditor) -> (BTreeMap<&'static str, String>, Vec<usize>, Vec<String>) {
    let mut g = BTreeMap::new();
    let v2 = |v: [f32; 2]| format!("{},{}", h(v[0]), h(v[1]));
    let v3 = |v: [f32; 3]| format!("{},{},{}", h(v[0]), h(v[1]), h(v[2]));
    let q = |v: [f32; 4]| format!("{},{},{},{}", h(v[0]), h(v[1]), h(v[2]), h(v[3]));
    g.insert("wmp", v2(e.WorldMousePos));
    let cv = e.context_menu.canvas(e.ui.screen);
    g.insert("mpar", format!("{},{},{}", v3([cv.t[0], cv.t[1], cv.t[2]]), q(cv.q), v3([cv.s[0], cv.s[1], cv.s[2]])));
    g.insert("prev", v2(e.manipulator.previousMousePos));
    g.insert("locks", e.uiLockingRegistrations.to_string());
    let m = &e.manipulator;
    g.insert("move", format!("{},{},{}", bt(m.MoveGizmo.active), v3(m.MoveGizmo.position), q(m.MoveGizmo.rotation)));
    g.insert("resize", format!("{},{},{}", bt(m.ResizeGizmo.active), v3(m.ResizeGizmo.position), q(m.ResizeGizmo.rotation)));
    let hs = [m.AllAxisMoveGizmo, m.VerticalMoveGizmo, m.HorizontalMoveGizmo, m.TopResizeHandle, m.RightResizeHandle, m.LeftResizeHandle, m.BottomResizeHandle];
    g.insert("h", hs.iter().map(|x| format!("{},{},{},{}", bt(x.dragged), v3(x.position), q(x.rotation), v3(x.scale))).collect::<Vec<_>>().join("|"));
    g.insert("center", v2(m.CenterOfSelection));
    g.insert("sel", ids_text(&e.CurrentlySelected));
    g.insert("hov", ids_text(&e.HoveringOver));
    let cm = &e.context_menu;
    g.insert("menu", format!("{},{},{},{},{},{},{},{}", bt(cm.shown), v3(cm.position), bt(cm.DeleteInteractable), bt(cm.CopyInteractable), bt(cm.PasteInteractable), ids_text(&cm.selectablesWhenShown), ids_text(&cm.selectedObjects), ""));
    let sels = e.selectables.iter().map(|s| if s.alive { format!("depth={} age={} pos={} rot={} scale={}", s.Depth, s.selectionAge, v3(s.transform.position), q(s.transform.rotation), v3(s.transform.scale)) } else { "dead".into() }).collect();
    (g, e.selectables_in_order(), sels)
}

/// The recorded after state in `state`'s terms.
fn recorded(c: &Case) -> (BTreeMap<&'static str, String>, Vec<usize>, Vec<String>) {
    let g = kv(c.post_g);
    let mut out = BTreeMap::new();
    for k in ["wmp", "prev", "locks", "move", "resize", "h", "center", "sel", "hov"] {
        out.insert(k, g[k].to_string());
    }
    // The overlay canvas the menu hangs under (CanvasScaler's scale), where the recording has it.
    if let Some(m) = g.get("mpar") {
        out.insert("mpar", m.to_string());
    }
    let mv: Vec<&str> = g["menu"].split(',').collect();
    out.insert("menu", format!("{},{},{},{},{},{},{},{},{}", mv[0], mv[1], mv[2], mv[3], mv[9], mv[10], mv[11], mv[12], mv[13]) + ",");
    let sels = c.post_s.iter().map(|s| if s.live {
        let v3 = |v: [f32; 3]| format!("{},{},{}", h(v[0]), h(v[1]), h(v[2]));
        format!("depth={} age={} pos={} rot={},{},{},{} scale={}", s.depth, s.age, v3(s.pos), h(s.rot[0]), h(s.rot[1]), h(s.rot[2]), h(s.rot[3]), v3(s.scale))
    } else { "dead".into() }).collect();
    (out, c.post_order.clone(), sels)
}

/// The recorder's collider text (MapEditorProbe.Shape), '|' between colliders.
fn shapes(cols: &[Shape]) -> String {
    let v2 = |v: [f32; 2]| format!("{},{}", h(v[0]), h(v[1]));
    cols.iter().map(|c| match c {
        Shape::Box { center, size } => format!("B:{},{}", v2(*center), v2(*size)),
        Shape::Circle { center, radius } => format!("C:{},{}", v2(*center), h(*radius)),
        Shape::Polygon(p) => format!("P:{}", p.iter().map(|x| v2(*x)).collect::<Vec<_>>().join(";")),
    }).collect::<Vec<_>>().join("|")
}

/// The lifecycle cases' own results: the recorded after value and ours, for the fields the case's function writes.
fn lifecycle_diffs(e: &MapEditor, c: &Case) -> Vec<String> {
    let g = kv(c.post_g);
    let v3 = |v: [f32; 3]| format!("{},{},{}", h(v[0]), h(v[1]), h(v[2]));
    let (k, ours) = match c.name {
        "MapEditorGlobal.Awake" => ("uni", (e.CurrentUniverse as u8).to_string()),
        "MapEditorGlobal.Update" => (crate::il!(str "MapEditorGlobal::Update" 0), format!("{},{}", bt(e.PauseMenu), bt(e.Catalog))),
        "MapEditorManipulator.Awake" => ("lis", format!("{},{}", e.listeners[0], e.listeners[1])),
        "MapEditorPanelBehaviour.Update" => ("panel", format!("{},{}", v3(e.panel_scale), v3(e.panel_position))),
        _ => return vec![],
    };
    let game = g.get(k).copied().unwrap_or("");
    if canon(game) == canon(&ours) { vec![] } else { vec![format!("{k}: {}", elems(&canon(game), &canon(&ours)))] }
}

fn run(e: &mut MapEditor, input: &Input, c: &Case) -> Option<String> {
    let me = c.this;
    let void = || "void".to_string();
    Some(match c.name {
        "MapEditorGlobal.Awake" => { e.global_awake(); "1".into() }
        "MapEditorGlobal.Start" => { let main = e.Camera; e.Camera = EditorCamera::default(); e.global_start(main); bt(e.Camera.position == main.position && e.Camera.orthographicSize == main.orthographicSize).into() }
        "MapEditorGlobal.Update" => { e.global_update(input); void() }
        "MapEditorGlobal.get_IsPauseMenuOpen" => bt(e.IsPauseMenuOpen()).into(),
        "MapEditorGlobal.get_IsCatalogOpen" => bt(e.IsCatalogOpen()).into(),
        "MapEditorGlobal.get_WorldMousePos" => format!("{},{}", h(e.WorldMousePos[0]), h(e.WorldMousePos[1])),
        "MapEditorGlobal.set_WorldMousePos" => { e.WorldMousePos = c.screen; void() }
        "MapEditorManipulator.Awake" => { e.manipulator_awake(); void() }
        "MapEditorManipulator.Start" => { e.manipulator_start(); "1".into() }
        "MapEditorContextMenuBehaviour.Start" => { e.context_menu_hide(); void() }
        "MapEditorObjectBehaviour.Awake" => e.object_awake(me).map_or("null".into(), |x| x.to_string()),
        "MapEditorSelectable.Awake" => { let cols = c.pre_s[me].cols.clone()?; e.selectable_awake(me, cols); shapes(e.get_colliders(me)) }
        "MapEditorSelectable.GetColliders" => shapes(e.get_colliders(me)),
        "MapEditorPanelBehaviour.Update" => { e.panel_update(); void() }
        "MapEditorSelectable.MouseOverlaps" => bt(e.mouse_overlaps(me)).into(),
        "MapEditorSelectionManager.DetermineHovering" => { e.determine_hovering(); void() }
        "MapEditorSelectionManager.UpdateSortedSelectables" => format!("sorted:{}", ids_text(&e.UpdateSortedSelectables())),
        "MapEditorSelectionManager.get_TopMostHovering" => e.TopMostHovering().map_or("null".into(), |x| x.to_string()),
        "MapEditorSelectionManager.Select" => match e.Select(me) { Ok(()) => void(), Err(_) => "exc:Exception".into() },
        "MapEditorSelectionManager.Deselect" => { e.Deselect(me); void() }
        "MapEditorSelectable.OnDeselect" => { e.selectable_on_deselect(me); void() }
        "MapEditorSelectionManager.ClearSelection" => { e.ClearSelection(); void() }
        "MapEditorManipulator.HandleResizeSingleObject" => { e.handle_resize_single_object(me); void() }
        "MapEditorManipulator.HandleResizing" => { e.handle_resizing(); void() }
        "MapEditorManipulator.CalculateSelectionTransform" => { e.calculate_selection_transform(); void() }
        "MapEditorManipulator.OnSelect" => { e.manipulator_on_select(); void() }
        "MapEditorManipulator.OnDeselect" => { e.manipulator_on_deselect(); void() }
        "MapEditorManipulator.HandleTranslation" => { e.handle_translation(input); void() }
        "MapEditorManipulator.OnRenderObject" => { e.manipulator_on_render_object(input); void() }
        "MapEditorGlobal.RegisterUILock" => { e.RegisterUILock(); void() }
        "MapEditorGlobal.DeregisterUILock" => match crate::game::menu::catch_managed(|| e.DeregisterUILock()) { None => void(), Some(x) => format!("exc:{}", x.0.split(':').next().unwrap_or("")) },
        "MapEditorGlobal.get_UiLock" => bt(e.UiLock()).into(),
        "MapEditorContextMenuBehaviour.Show" => { e.context_menu_show(c.screen); void() }
        "MapEditorContextMenuBehaviour.Hide" => { e.context_menu_hide(); void() }
        "MapEditorContextMenuBehaviour.get_IsShown" => bt(e.context_menu_is_shown()).into(),
        "MapEditorContextMenuBehaviour.Update" => { e.context_menu_update(); void() }
        "MapEditorSelectionManager.Update" => { e.selection_update(input); void() }
        "MapEditorObjectBehaviour.Delete" => { e.object_delete(me); e.flush_destroyed(); void() }
        "MapEditorContextMenuBehaviour.DeleteAction" => { e.context_menu_delete_action(); e.flush_destroyed(); void() }
        _ => return None,
    })
}

pub fn fn_check(path: &std::path::Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut tally: BTreeMap<String, (usize, usize, Option<String>)> = BTreeMap::new();
    let mut deaths: Vec<usize> = Vec::new();
    let mut start_checked = false;
    // MapRegistry.Register (MapLoadStep.Load's, every built-in map) and MapRegistry.GetAllMaps: the game's map list.
    if let Some(line) = text.lines().find_map(|l| l.strip_prefix("maps ")) {
        let game = crate::game_assets::find_game(None)?;
        let ours = crate::ui::data::maps(&game)?.into_iter().map(|m| m.name).collect::<Vec<_>>().join("|");
        let ok = ours == line;
        for n in ["MapRegistry.Register", "MapRegistry.GetAllMaps"] {
            tally.insert(n.into(), if ok { (1, 0, None) } else { (0, 1, Some(format!("maps: game {line} ours {ours}"))) });
        }
    }
    for c in cases(&text) {
        tally.entry(c.name.to_string()).or_default();
        // MapEditorSelectable.Start's LocalBounds, from the colliders (once, on the first case's live selectables).
        if !start_checked {
            start_checked = true;
            let st = tally.entry("MapEditorSelectable.Start".into()).or_default();
            match build(&c, &deaths, false) {
                Ok((e, _)) => for (i, s) in c.pre_s.iter().enumerate().filter(|x| x.1.live) {
                    let lb = &e.selectables[i].LocalBounds;
                    let ours = format!("{},{},{},{}", h(lb.center[0]), h(lb.center[1]), h(lb.extents[0]), h(lb.extents[1]));
                    let game = format!("{},{},{},{}", h(s.lb[0]), h(s.lb[1]), h(s.lb[3]), h(s.lb[4]));
                    if ours == game { st.0 += 1 } else { st.1 += 1; st.2.get_or_insert(format!("selectable {i} LocalBounds: game {game} ours {ours}")); }
                },
                Err(e) => { st.1 += 1; st.2.get_or_insert(e); }
            }
        }
        let (mut e, input) = match build(&c, &deaths, true) {
            Ok(x) => x,
            Err(err) => { let t = tally.get_mut(c.name).unwrap(); t.1 += 1; t.2.get_or_insert(format!("case {}: {err}", c.k)); continue }
        };
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { let r = run(&mut e, &input, &c); (r, e) }));
        let t = tally.get_mut(c.name).unwrap();
        let (ret, e) = match r {
            Ok((Some(ret), e)) => (ret, e),
            Ok((None, _)) => { t.1 += 1; t.2.get_or_insert("no rewrite function listed".into()); continue }
            Err(p) => { t.1 += 1; t.2.get_or_insert(format!("case {}: panic: {}", c.k, p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default())); continue }
        };
        let mut diffs = Vec::new();
        let game_ret = match &c.sorted { Some(s) => format!("sorted:{}", ids_text(s)), None => c.ret.to_string() };
        if ret != game_ret {
            diffs.push(format!("return: game {game_ret} ours {ret}"));
        }
        let (og, oo, os) = state(&e);
        let (rg, ro, rs) = recorded(&c);
        // After Delete's frame the editor's own Updates ran: only what Delete and OnDestroy change is compared.
        let delete = matches!(c.name, "MapEditorObjectBehaviour.Delete" | "MapEditorContextMenuBehaviour.DeleteAction");
        for (k, v) in &rg {
            if delete && !matches!(*k, "sel") {
                continue;
            }
            let (ours, v) = (og.get(k).cloned().unwrap_or_default(), v.clone());
            let (ours, v) = if *k == "h" { (move_handles_drag_only(&ours), move_handles_drag_only(&v)) } else { (ours, v) };
            if canon(&ours) != canon(&v) {
                diffs.push(format!("{k}: {}", elems(&canon(&v), &canon(&ours))));
            }
        }
        if delete {
            for k in ["move", "resize", "menu"] {
                if og[k].split(',').next() != rg[k].split(',').next() {
                    diffs.push(format!("{k}.active: game {} ours {}", rg[k], og[k]));
                }
            }
        }
        diffs.extend(lifecycle_diffs(&e, &c));
        if oo != ro {
            diffs.push(format!("order: game {} ours {}", ids_text(&ro), ids_text(&oo)));
        }
        for (i, (a, g)) in os.iter().zip(&rs).enumerate() {
            let (a, g) = if delete { (a.split(' ').next().unwrap_or("") == "dead", g.split(' ').next().unwrap_or("") == "dead") } else { (canon(a) == canon(g), true) };
            if delete && a != g || !delete && !a {
                diffs.push(format!("selectable {i}: game {} ours {}", rs[i], os[i]));
            }
        }
        if delete {
            deaths.push(c.this);
        }
        let msg = format!("case {} self={}: {}", c.k, c.this, diffs.join(" | "));
        // MapEditorSelectable.OnDestroy is the end-of-frame half of each Delete (Selectables, CurrentlySelected, OnDeselect).
        for n in if delete { vec![c.name, "MapEditorSelectable.OnDestroy"] } else { vec![c.name] } {
            let t = tally.entry(n.to_string()).or_default();
            if diffs.is_empty() { t.0 += 1 } else { t.1 += 1; t.2.get_or_insert(msg.clone()); }
        }
    }
    let mut pass = 0;
    for (name, (p, fl, first)) in &tally {
        if *fl == 0 && *p > 0 {
            pass += 1;
            println!("PASS {name} {p}");
        } else {
            println!("FAIL {name} {fl}/{}: {}", p + fl, first.clone().unwrap_or_default());
        }
    }
    println!("fn-check: {pass} of {} functions pass", tally.len());
    Ok(())
}
