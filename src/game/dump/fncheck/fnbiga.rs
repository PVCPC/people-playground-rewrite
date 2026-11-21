//! Hand entries for big game methods (tools/progress/big-a.tsv) whose recorder cases come from tools/recorder/BigProbeA.cs
//! (PPG_FNPROBE=biga:..). Looked up before the generated table.
use super::*;

/// A string return as BigProbeA writes it: \r, \n, \t escaped, spaces as underscores, quoted.
fn text(s: String) -> String {
    quoted(&s.replace('\r', "\\r").replace('\n', "\\n").replace('\t', "\\t"))
}

fn hx(s: &str) -> Result<f32, String> {
    u32::from_str_radix(s, 16).map(f32::from_bits).map_err(|_| format!("not a hex float: {s}"))
}

fn int(s: &str) -> Result<i32, String> {
    s.parse().map_err(|_| format!("not an int: {s}"))
}

fn hex8(v: f32) -> String {
    format!("{:08x}", v.to_bits())
}

/// biga:legacyprefs' PlayerPrefs: the case's strings and floats.
#[derive(Default)]
struct Table(std::collections::HashMap<String, String>, std::collections::HashMap<String, f32>);

impl crate::game::player_prefs::PlayerPrefsStore for Table {
    fn string(&self, key: &str) -> Option<String> { self.0.get(key).cloned() }
    fn float(&self, key: &str) -> Option<f32> { self.1.get(key).copied() }
    fn has_key(&self, key: &str) -> bool { self.0.contains_key(key) || self.1.contains_key(key) }
}

/// biga:toolbar's strings: UTF-8 bytes in hex, "-" for null (read as "", what ToolLibrary's serialized strings hold).
fn unhex(s: &str) -> Result<String, String> {
    if s == "-" {
        return Ok(String::new());
    }
    let b = (0..s.len()).step_by(2).map(|i| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok())).collect::<Option<Vec<u8>>>().ok_or_else(|| format!("not hex: {s}"))?;
    String::from_utf8(b).map_err(|e| e.to_string())
}

/// biga:toolbar's walk of a made button (BigProbeA.TDesc): "Name{Text}click=Type", then for a child container its
/// activeSelf at first and after OnMouseEnter, each child's walk (whose click hides this container) and this container
/// after it, and the container after OnMouseExit.
fn toolbar_walk(b: &crate::ui::view::ToolButton, fl: &mut crate::ui::view::Flyouts, parent: Option<&str>) -> String {
    let mut s = format!("{}{{{}}}click={}", b.tool.name, b.text, b.tool.ty);
    if let Some(p) = parent {
        fl.clicked_in(p);
    }
    if !b.children.is_empty() {
        let name = &b.tool.name;
        let a = |fl: &crate::ui::view::Flyouts| if fl.open(name) { "1" } else { "0" };
        s += &format!(" c{}", a(fl));
        fl.hover(name, true);
        s += a(fl);
        s += "[";
        for k in &b.children {
            s += &toolbar_walk(k, fl, Some(name));
            s += &format!(",{};", a(fl));
        }
        s += "]";
        fl.hover(name, false);
        s += a(fl);
    }
    s
}

pub(super) const CALLS: &[(&str, Call)] = &[
    // biga:toolbar: CreateButton(list[index], a temporary container, list) on the scene's ToolBarGenerator; the list is the
    // case's (the real Tools or Powers, or a seeded one). The return walks the made buttons (toolbar_walk) through the
    // view's own spec and flyout state.
    ("ToolBarGenerator.CreateButton", |_g, c| {
        let w = c.with(2)?;
        let i = w.p_int::<usize>(0)?;
        let list = w.p_string(1)?.split('|').map(|e| {
            let f: Vec<&str> = e.split(':').collect();
            let [name, description, ty, parent] = f[..] else { return Err(format!("bad tool {e}")) };
            Ok(crate::ui::art::ToolDef { name: unhex(name)?, description: unhex(description)?, ty: unhex(ty)?, parent: unhex(parent)? })
        }).collect::<Result<Vec<_>, String>>()?;
        let tool = list.get(i).ok_or("tool index outside the list")?;
        let b = crate::ui::view::tool_button_spec(tool, &list);
        Ok(text(toolbar_walk(&b, &mut Default::default(), None)))
    }),
    // biga:handles: the scene's handles with the case's Targets (each one's Resizable), HandleDistanceMultiplier,
    // multiplyingScale, orthographicSize and "fast"; the return is the handles' state after, or "same" when none of it changed.
    ("ResizeHandles.HandleDisplay", |g, c| {
        let w = c.with(6)?;
        g.recorded_resize_handles()?;
        let targets = w.p_list(0, |s| w.item_usize(g, s))?;
        for (&t, f) in targets.iter().zip(w.p_string(1)?.split(',')) {
            g.physicals[t].Resizable = f == "1";
        }
        g.resize.Targets = targets;
        g.resize.HandleDistanceMultiplier = w.p_arr::<2>(2)?;
        g.resize.multiplying_scale = w.p_f32(3)?;
        g.camera.orthographic_size = w.p_f32(4)?;
        if w.p_bool(5)? {
            g.input.controls.insert("fast".into(), crate::game::tool::Control { held: true, ..Default::default() });
        }
        let state = |g: &Game| g.resize_handle_state().into_iter().map(hex8).collect::<Vec<_>>().join(",");
        let before = state(g);
        g.resize_handle_display();
        let after = state(g);
        Ok(quoted(if after == before { "same" } else { &after }))
    }),
    // biga:asynctask: a static method (no loader state); the action counts its runs, sees the overlay while it runs, and fails
    // for kinds 1 (a faulted task) and 2 (a throw). The return is "runs|during|after", the overlay as "active,name,state".
    ("BackgroundItemLoader.DoAsyncTask", |_g, c| {
        let w = c.with(3)?;
        let (name, desc, kind) = (w.p_string(0)?.replace('_', " "), w.p_string(1)?.replace('_', " "), w.p_int::<i32>(2)?);
        let shown = |s: &crate::game::background_loader::Status| match &s.shown {
            Some(n) => format!("1,{n},{}", s.state),
            None => format!("0,-,{}", s.state),
        };
        let mut l = crate::game::background_loader::BackgroundItemLoader::new();
        let (mut runs, mut during) = (0, String::new());
        l.DoAsyncTask(|s| {
            runs += 1;
            during = shown(s);
            if matches!(kind, 1 | 2) { Err("faulted".into()) } else { Ok(()) }
        }, &name, &desc);
        Ok(quoted(&format!("{runs}|{during}|{}", shown(&l.status))))
    }),
    // biga:skinrender: GetComputedColor's cache (seeded by the args) is the simulation; the return is that cache after. The
    // material floats and bloodColor only feed the shader.
    ("SkinMaterialHandler.OnWillRenderObject", |g, c| {
        let w = c.with(2)?;
        let l = c.comp("SkinMaterialHandler")?;
        let o = crate::game::container::Owner::Circulation(l);
        (g.bc_mut(o).isComputedColourCached, g.bc_mut(o).cachedComputedColour) = (w.p_bool(0)?, w.p_arr::<4>(1)?);
        g.skin_will_render(l);
        let b = g.bc_mut(o);
        Ok(quoted(&format!("{},{}", b.isComputedColourCached as u8, b.cachedComputedColour.map(hex8).join(","))))
    }),
    // CameraControlBehaviour's state is not in a snapshot: the case's args set it (biga:camera), the return reads it back.
    ("CameraControlBehaviour.LateUpdate", |g, c| {
        let w = c.with(8)?;
        let v: Vec<f32> = (0..7).map(|j| w.p_f32(j)).collect::<Result<_, _>>()?;
        let follow = w.p_list(7, |s| w.item_usize(g, s))?;
        let cc = &mut g.camera_control;
        cc.started = true;
        cc.targetZoom = v[0];
        cc.targetPos = [v[1], v[2], v[3]];
        cc.position = [v[4], v[5], v[6]];
        cc.CurrentlyFollowing = follow;
        g.camera_control_late_update();
        let cc = &g.camera_control;
        let b = |x: f32| format!("{:08x}", x.to_bits());
        let mut r: Vec<String> = [cc.targetZoom, cc.targetPos[0], cc.targetPos[1], cc.targetPos[2], cc.position[0], cc.position[1], cc.position[2], g.camera.orthographic_size].map(b).to_vec();
        r.push(cc.CurrentlyFollowing.len().to_string());
        r.push(if g.following_view_active { "1" } else { "0" }.into());
        Ok(quoted(&r.join(",")))
    }),
    // A Cable or Energy wire laid by the wire tool (biga:wirestart); self is the wire.
    ("DistanceJointWireBehaviour.Start", |g, c| { let w = c.with(1)?; let x = w.p_usize(g, 0)?; g.distance_joint_wire_start(x); VOID }),
    // A wire the step after an earlier case destroyed (self id 0, fnba12's odd cases from 7): Start on a destroyed component throws.
    ("SpringCableBehaviour.Start", |g, c| { if c.sid == 0 { return Ok("exc:NullReferenceException".into()) } let w = c.with(1)?; let x = w.p_usize(g, 0)?; g.spring_cable_start(x); VOID }),
    // biga:emit: Emit with the case's args (width and color only draw the line); onHit null or the probe's hook, whose hit is the return.
    ("SimpleLightningEmitterBehaviour.Emit", |g, c| {
        let w = c.with(11)?;
        let hit = if w.p_bool(4)? { crate::game::lightning::LightningHit::Probe } else { crate::game::lightning::LightningHit::None };
        g.fn_lightning_hit = None;
        g.lightning_emit(w.p_arr::<2>(0)?, w.p_arr::<2>(1)?, w.p_f32(2)?, w.p_f32(7)?, w.p_int(8)?, w.p_f32(9)?, w.p_f32(10)?, w.p_int::<i32>(6)? as u32, hit);
        Ok(quoted(&g.fn_lightning_hit.map_or("-".into(), |(p, v)| format!("{},{},{}", g.physicals[p].id, hex8(v[0]), hex8(v[1])))))
    }),
    // biga:legacyprefs: the case's PlayerPrefs table (key=s:text or key=f:hex, | between, _ for space); the return is the kept fields.
    ("UserPreferenceManager.LoadFromLegacyStorage", |_g, c| {
        let w = c.with(1)?;
        let mut t = Table::default();
        for e in w.p_string(0)?.split('|').filter(|s| !s.is_empty()) {
            let (k, v) = e.split_once('=').ok_or("not key=value")?;
            match v.split_at(2) {
                ("s:", s) => { t.0.insert(k.into(), s.replace('_', " ")); }
                ("f:", h) => { t.1.insert(k.into(), hx(h)?); }
                _ => return Err(format!("bad entry {e}")),
            }
        }
        let p = crate::game::player_prefs::legacy_preferences(&t);
        let b = |x: bool| if x { "1" } else { "0" };
        Ok(quoted(&[b(p.DropOnDeath).into(), b(p.LimbCrushing).into(), b(p.TracerBullets).into(), b(p.FancyEffects).into(), b(p.Decals).into(), hex8(p.CrushForceMultiplier), hex8(p.ShakeIntensity), p.CollisionQuality.to_string(), p.PhysicsIterations.to_string()].join(",")))
    }),
    // A fresh controller with the case's rates and SetCell pairs (biga:radgrid); the return is every cell after, by x then y.
    ("RadiationGridController.UpdateGrid", |_g, c| {
        let w = c.with(5)?;
        let mut r = crate::game::grid::RadiationGrid::new(0);
        (r.VelocityRetainment, r.ValueDropRate, r.ValueTransferRate) = (hx(&w.p_string(1)?)?, hx(&w.p_string(2)?)?, hx(&w.p_string(3)?)?);
        for cell in w.p_string(4)?.split('|').filter(|s| !s.is_empty()) {
            let v: Vec<&str> = cell.split(':').collect();
            let k = (int(v[0])?, int(v[1])?);
            r.set_cell(k, hx(v[2])?);
            r.set_cell(k, hx(v[3])?);
        }
        r.update_grid();
        let mut cells: Vec<_> = r.grid.iter().collect();
        cells.sort_by_key(|(k, _)| **k);
        Ok(text(cells.iter().map(|(k, c)| format!("{}:{}:{}:{}:{}", k.0, k.1, hex8(c.PreviousValue), hex8(c.Value), hex8(c.Velocity))).collect::<Vec<_>>().join("|")))
    }),
    // A fresh World with the case's cells, the map's box and AmbientTemperature (biga:tempgrid); the return, then every cell after.
    ("AmbientTemperatureGridBehaviour.GetAverageNeighbouringTemperature", |g, c| {
        let w = c.with(7)?;
        g.AmbientTemperature = hx(&w.p_string(1)?)?;
        let pair = |s: String| -> Result<[f32; 2], String> { let (a, b) = s.split_once(',').ok_or("not a pair")?; Ok([hx(a)?, hx(b)?]) };
        (g.BoundingBox.center, g.BoundingBox.extents) = (pair(w.p_string(2)?)?, pair(w.p_string(3)?)?);
        let v = (int(&w.p_string(4)?)?, int(&w.p_string(5)?)?);
        g.ambient_grid.World = Default::default();
        let mut centre = None;
        for cell in w.p_string(6)?.split('|').filter(|s| !s.is_empty()) {
            let f: Vec<&str> = cell.split(':').collect();
            let k = (int(f[0])?, int(f[1])?);
            let mut x = crate::game::temperature::Cell::new(hx(f[2])?);
            (x.WriteTemperature, x.CanCreateNeighbours, x.CanTransferTemperature) = (hx(f[3])?, f[4] == "1", f[5] == "1");
            let i = g.ambient_grid.World.try_add(k, x);
            if k == v {
                centre = Some(i);
            }
        }
        let t = g.get_average_neighbouring_temperature(centre.ok_or("no centre cell")?, v);
        let mut cells = Vec::new();
        let mut e = g.ambient_grid.World.begin_enumeration();
        while let Some((k, i)) = g.ambient_grid.World.move_next(&mut e) {
            cells.push((k, i));
        }
        cells.sort_by_key(|x| x.0);
        let world = &g.ambient_grid.World;
        let rows = cells.iter().map(|&(k, i)| { let x = world.cell(i); format!("{}:{}:{}:{}:{}:{}", k.0, k.1, hex8(x.ReadTemperature), hex8(x.WriteTemperature), x.CanCreateNeighbours as u8, x.CanTransferTemperature as u8) });
        Ok(text(std::iter::once(hex8(t)).chain(rows).collect::<Vec<_>>().join("|")))
    }),
    // As LimbBehaviour.Crush calls it (biga:generate): args the limb, which of its generators (0 bone, 1 flesh, 2 skin), the
    // creation buffer's length (PartCount), ConnectToRange, Brightness.
    ("ShatteredObjectGenerator.Generate", |g, c| {
        let w = c.with(6)?;
        let (p, k, n) = (w.p_usize(g, 1)?, w.p_usize(g, 2)?, w.p_usize(g, 3)?);
        let l = g.limbs.iter().position(|x| x.PhysicalBehaviour == p).ok_or("not a limb")?;
        let gi = g.limbs[l].ShatteredObjectGenerators.and_then(|x| x[k]).ok_or("no such generator")?;
        // The live ConnectToRange and Brightness (the prefab's are stale once the limb has shattered).
        g.shatter_generators[gi].g.ConnectToRange = w.p_f32(4)?;
        g.shatter_generators[gi].g.Brightness = w.p_f32(5)?;
        let (count, buffer) = g.limb_generator_generate(l, gi);
        if buffer.len() != n { return Err(format!("buffer {} not {n}", buffer.len())) }
        Ok(count.to_string())
    }),
    // biga:global: the args set what Global.Update reads (none of it is in a snapshot), the return reads back what it leaves.
    ("Global.Update", |g, c| {
        let w = c.with(25)?;
        let keys = w.p_string(0)?;
        for k in keys.split(',').filter(|k| *k != "-") {
            let on = crate::game::tool::Control { down: true, ..Default::default() };
            if k == "F1" { g.input.keys.insert(k.into(), on); } else { g.input.controls.insert(k.into(), on); }
        }
        g.steam_off = !w.p_bool(24)?;
        g.ShowLimbStatus = w.p_bool(22)?;
        g.catalog_selected = Some(w.p_string(23)?.replace('_', " "));
        g.inactivity = w.p_f32(1)?;
        crate::game::stats::set_last_kill_time(g.time.time - w.p_f32(2)?);
        g.alreadyUnlockedPacifist = w.p_bool(3)?;
        g.time.timeScale = w.p_f32(4)?;
        g.timeScaleTarget.0 = w.p_f32(5)?;
        g.SlowMotion = w.p_bool(6)?;
        g.paused = w.p_bool(7)?;
        g.paused_menu = w.p_bool(8)?;
        g.menu_page = w.p_string(9)?;
        g.time.unscaledDeltaTime = w.p_f32(10)?;
        g.ActiveUiBlockers = w.p_int(11)?;
        g.dialog_open = w.p_bool(12)?;
        g.trigger_being_edited = w.p_bool(13)?;
        g.prefs.GridSnap = w.p_f32(14)?;
        g.prefs.AngleSnap = w.p_int(15)?;
        g.prefs.ShowOutlines = w.p_bool(16)?;
        g.prefs.SlowMotionSpeed = w.p_f32(17)?;
        g.input.mouse = w.p_arr::<3>(18)?;
        // The menu's position as Show clamps it: the pointer (Ignite reads it).
        g.context_at = [g.input.mouse[0], g.input.mouse[1]];
        let hover = w.p_opt_usize(g, 19)?;
        let sel = w.p_list(20, |s| w.item_usize(g, s))?;
        g.resize.active = w.p_bool(21)?;
        // SelectionController.Main's hover and selection (a snapshot game has no tool state).
        let t = g.tool.get_or_insert_with(Default::default);
        t.hover = hover;
        t.rawSelection = sel;
        g.global_update();
        let b = |x: bool| if x { "1" } else { "0" };
        let h = |x: f32| format!("{:08x}", x.to_bits());
        let r = [
            b(g.paused_menu).into(), if g.paused_menu { g.menu_page.replace(' ', "_") } else { "-".into() }, b(g.SlowMotion).into(), h(g.timeScaleTarget.0), h(g.time.timeScale),
            b(g.paused).into(), b(g.ShowLimbStatus).into(), h(g.prefs.GridSnap), g.prefs.AngleSnap.to_string(), b(g.prefs.ShowOutlines).into(), b(g.alreadyUnlockedPacifist).into(),
            b(crate::game::stats::last_kill_time() == g.time.time).into(), g.MousePosition.map(h).join(","), g.catalog_selected.clone().unwrap_or("null".into()).replace(' ', "_"),
            g.notifications.join("|").replace(' ', "_").replace(',', "."),
        ];
        Ok(quoted(&r.join(",")))
    }),
    // biga:ignoreparent: an explosive's Killzone, inactive under the item (ExistInDetailView.Start with the detail view off),
    // so its own Start has not run. Args: the seeded parent ActOnDestroy (0 as spawned, 1 DestroyWith null, 2 an array without
    // the object, 3 one holding it; 4 as spawned and the object taken out of the item by SetParent(null) before the call) and
    // DestroyWith's Transform ids (':' between, '-' none). A direct call runs the body on the inactive object and has no
    // once-guard. The return is what the snapshot lacks: the parent, localScale, the old parent's ActOnDestroy (count,
    // DestroyWith), the ParentConstraint's sources (Transform id/weight) and constraintActive.
    ("IgnoreParentSize.Start", |g, c| {
        let w = c.with(2)?;
        let (v, list) = (w.p_int::<i32>(0)?, w.p_string(1)?);
        let ids = list.split(':').filter(|s| *s != "-" && !s.is_empty()).map(int).collect::<Result<Vec<_>, _>>()?;
        let i = g.ignore_parent_sizes.iter().position(|x| x.id == c.sid).ok_or_else(|| format!("no ignore_parent_sizes entry (have {:?})", g.ignore_parent_sizes.iter().map(|x| x.id).collect::<Vec<_>>()))?;
        g.ignore_parent_sizes[i].started = false;
        let (key, _, own) = g.ignore_parent_size_parent(c.sid).ok_or("the Killzone has no parent")?;
        let root = g.with_component_instance(c.sid, |_, inst, o| inst.objects[o].components[0]);
        match v {
            0 | 4 => {}
            1..=3 => {
                g.act_on_destroy_get_or_add(key);
                g.act_on_destroy.get_mut(&key).unwrap().destroy_with = ids;
            }
            _ => return Err(format!("bad variant {v}")),
        }
        if v == 4 {
            if own { g.set_parent_null_of(c.sid, |_, _, _| {}) } else { g.attached_clone_set_parent_null(root) }
        }
        g.ignore_parent_size_start_body(i);
        let parent = g.ignore_parent_size_parent(c.sid).map_or("null".into(), |x| x.0.to_string());
        let (scale, pc) = g.with_component_instance(c.sid, |_, inst, o| {
            let pc = inst.prefab.objects[o].components.iter().position(|(_, x)| matches!(x, crate::assets::db::Component::ParentConstraint(_))).map(|k| inst.objects[o].components[k]);
            (inst.objects[o].trs.s, pc)
        });
        let aod = match g.act_on_destroy.get(&key) {
            Some(a) => format!("1;{}", a.destroy_with.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(":")),
            None => "0;-".into(),
        };
        let pc = pc.and_then(|pc| g.parent_constraints.iter().rev().find(|x| x.id == pc));
        let sources = pc.map_or(Vec::new(), |p| p.sources.iter().map(|s| {
            let t = match s.transform { crate::game::constraint::ConstraintSource::Physical(p) => g.physicals[p].transform_id, crate::game::constraint::ConstraintSource::Object(k) => k };
            format!("{t}/{}", hex8(s.weight))
        }).collect());
        let active = pc.is_some_and(|p| p.active) as u8;
        Ok(quoted(&format!("{parent};{},{},{};{aod};{}:{};{active}", hex8(scale[0]), hex8(scale[1]), hex8(scale[2]), sources.len(), sources.join(":"))))
    }),
    // Called again on a live limb (biga:start); self is the PhysicalBehaviour.
    ("PhysicalBehaviour.Start", |g, c| { let w = c.with(1)?; let p = w.p_usize(g, 0)?; super::super::sliding_loop_clip(g, g.physicals[p].Properties)?; g.physical_start(p); VOID }),
    // The Collision2D is parsed (the game's method never reads it); self is the FireballBehaviour.
    ("FireballBehaviour.OnCollisionEnter2D", |g, c| {
        c.with(1)?.p_collision(0)?;
        let x = g.fireballs.iter().position(|x| x.id == c.sid || x.id == c.id).ok_or("no fireballs entry")?;
        g.fireball_on_collision_enter(x);
        VOID
    }),
    // LimbStatusViewBehaviour's texts: no parameters; their input, this.Limbs, is the case's one arg (biga:limbview).
    ("LimbStatusViewBehaviour.GetBloodInformation", |g, c| { let c = c.with(1)?; let l = c.p_list(0, |s| c.item_usize(g, s))?; Ok(text(g.limb_view_blood(&l))) }),
    ("LimbStatusViewBehaviour.GetSourceStats", |g, c| { let c = c.with(1)?; let l = c.p_list(0, |s| c.item_usize(g, s))?; Ok(text(g.limb_view_sources(&l))) }),
    ("LimbStatusViewBehaviour.GetLimbState", |g, c| { let c = c.with(1)?; let l = c.p_list(0, |s| c.item_usize(g, s))?; Ok(text(g.limb_view_state(&l))) }),
];
