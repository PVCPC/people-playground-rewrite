//! Scan of every ParticleSystem / ParticleSystemRenderer / ParticleSystemForceField / WindZone / LineRenderer of the
//! shipped build for the settings that reach a particle-side `unimplemented!` (tools/scan_ps.py runs it and prints the
//! counts; a `mod-only:` stop needs its count 0): `cargo test stop_scan -- --ignored --nocapture`.
use crate::assets::components::ParticleRenderer;
use crate::assets::db::{AssetDb, ObjRef};
use crate::assets::particles::*;
use std::collections::{BTreeMap, BTreeSet};

fn reach(f: impl FnOnce() + std::panic::UnwindSafe) -> Option<String> {
    std::panic::catch_unwind(f).err().map(|e| e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default())
}

fn all_curves(p: &ParticleSystem) -> Vec<&MinMaxCurve> {
    let mut v: Vec<&MinMaxCurve> = vec![&p.start_delay, &p.start_lifetime, &p.start_speed, &p.start_size, &p.start_rotation, &p.gravity, &p.rate_over_time, &p.rate_over_distance];
    v.extend(p.bursts.iter().map(|b| &b.count));
    if let Some(s) = &p.size {
        v.extend([&s.x, &s.y, &s.z]);
    }
    v.extend(p.rotation.iter());
    if let Some(s) = &p.sheet {
        v.extend([&s.frame_over_time, &s.start_frame]);
    }
    if let Some(x) = &p.velocity {
        v.extend(x.axes.iter().chain(&x.orbital).chain(&x.offset));
        v.extend([&x.radial, &x.speed_modifier]);
    }
    if let Some(x) = &p.inherit {
        v.push(&x.1);
    }
    if let Some(x) = &p.lifetime_by_speed {
        v.push(&x.0);
    }
    if let Some(x) = &p.force {
        v.extend(x.axes.iter());
    }
    if let Some(x) = &p.external_forces {
        v.push(&x.multiplier);
    }
    if let Some(x) = &p.clamp {
        v.extend([&x.magnitude, &x.drag]);
    }
    if let Some(x) = &p.noise {
        v.extend(x.strength.iter().chain(&x.remap).chain(&x.amounts));
        v.push(&x.scroll_speed);
    }
    if let Some(x) = &p.size_by_speed {
        v.extend([&x.0.x, &x.0.y, &x.0.z]);
    }
    if let Some(x) = &p.rotation_by_speed {
        v.push(&x.0);
    }
    if let Some(x) = p.collision.as_ref().filter(|c| c.enabled) {
        v.extend([&x.dampen, &x.bounce, &x.lifetime_loss]);
    }
    if let Some(x) = &p.trail {
        v.extend([&x.lifetime, &x.width_over_trail]);
    }
    v
}

#[test]
#[ignore]
fn stop_scan() {
    let Ok(game) = crate::game_assets::find_game(None) else { return };
    let mut db = AssetDb::open(&game).unwrap();
    std::panic::set_hook(Box::new(|_| {}));
    let out: std::cell::RefCell<BTreeMap<String, (usize, Vec<String>)>> = Default::default();
    let hit = |k: String, name: &str| {
        let mut o = out.borrow_mut();
        let e = o.entry(k).or_default();
        e.0 += 1;
        if e.1.len() < 8 {
            e.1.push(name.to_string());
        }
    };
    let mut systems = Vec::new();
    let mut children = BTreeSet::new();
    for (fi, f) in db.files.iter().enumerate() {
        for o in f.objects.iter().filter(|o| o.class_id == 198) {
            let r = ObjRef { file: fi, path_id: o.path_id };
            let p = ParticleSystem::read(f.data(o)).unwrap();
            let go = crate::assets::components::game_object_of(f.data(o)).ok().and_then(|g| db.resolve(fi, g));
            let name = go.and_then(|g| db.game_object(g).ok()).map_or("?".into(), |g| g.name);
            let rend = go
                .and_then(|g| db.game_object(g).ok())
                .and_then(|g| g.components.into_iter().filter_map(|c| db.resolve(fi, c)).find(|c| db.class(*c) == 199))
                .and_then(|c| ParticleRenderer::read(db.data(c)).ok());
            for s in &p.subs {
                if let Some(t) = db.resolve(fi, s.emitter) {
                    children.insert(db.game_object(t).ok().map(|_| t).unwrap_or(t));
                }
            }
            systems.push((r, f.path.file_name().unwrap().to_string_lossy().to_string(), name, p, rend));
        }
    }
    let total = systems.len();
    for (r, file, name, p, rend) in &systems {
        let n = format!("{file}:{name}");
        let n = n.as_str();
        let child = children.contains(r);
        let h = |k: &str, c: bool| {
            if c {
                hit(k.to_string(), n)
            }
        };
        let pr = std::panic::catch_unwind(|| p.supports_procedural());
        let proc_ = match &pr {
            Ok(b) => *b,
            Err(e) => {
                hit(format!("supports_procedural panics: {:?}", e.downcast_ref::<String>().cloned().unwrap_or_default()), n);
                false
            }
        };
        let state = |c: &MinMaxCurve| !matches!(c.state, 0 | 3);
        h("sub kind not 0|2", p.subs.iter().any(|s| !matches!(s.kind, 0 | 2)));
        h("sub kind 1 (collision)", p.subs.iter().any(|s| s.kind == 1));
        h("sprite-shaped (20) system with scaling 2", p.shape_enabled && p.shape_type == 20 && p.scaling > 1);
        h("space 2 (custom)", p.space == 2);
        h("collision enabled in custom space", p.space == 2 && p.collision.as_ref().is_some_and(|c| c.enabled));
        h("startDelay state not 0|3", state(&p.start_delay));
        let col_bad = |g: &MinMaxGradient| !(g.state == 1 && g.max.mode == 0 || g.state == 3 && g.max.mode == 0 && g.min.mode == 0);
        h("sub inherits colour, colour module not gradient/two-gradient blend", p.subs.iter().any(|s| s.properties & 1 != 0) && p.color.as_ref().is_some_and(col_bad));
        h(
            "sub inherits size, size module curve state not 0|1",
            p.subs.iter().any(|s| s.properties & 2 != 0) && p.size.as_ref().is_some_and(|s| [&s.x, &s.y, &s.z].iter().any(|c| !matches!(c.state, 0 | 1))),
        );
        h("sub inherits colour with colour by speed", p.subs.iter().any(|s| s.properties & 1 != 0) && p.color_by_speed.is_some());
        h("sub inherits size with size by speed", p.subs.iter().any(|s| s.properties & 2 != 0) && p.size_by_speed.is_some());
        h("sub inherits rotation with 3D start rotation", p.subs.iter().any(|s| s.properties & 4 != 0) && p.start_rotation_xy.is_some());
        h("unscaled-time sub system", child && p.unscaled_time);
        h("sprite-shaped (20) sub system", child && p.shape_enabled && p.shape_type == 20);
        h("lights module with a light prefab", p.lights && !p.lights_light.is_null());
        h(
            "procedural: start lifetime/speed/velocity curve state not 0|3",
            proc_ && (state(&p.start_lifetime) || state(&p.start_speed) || p.velocity.as_ref().is_some_and(|v| v.axes.iter().any(state))),
        );
        h("procedural: shape type not 10|12|18", proc_ && p.shape_enabled && !matches!(p.shape_type, 10 | 12 | 18));
        h("procedural: force module", proc_ && p.force.is_some());
        if let Some(sh) = p.sheet.as_ref().filter(|s| s.mode == 1) {
            h("sprite sheet with an empty sprite list", sh.sprites.is_empty());
            for sp in &sh.sprites {
                let s = db.resolve(r.file, *sp).and_then(|t| Some((t, db.sprite(t).ok()?)));
                h("sprite sheet entry null/unreadable", s.is_none());
                if let Some((t, s)) = s {
                    h("sprite sheet entry without texture", db.resolve(t.file, s.texture).is_none());
                }
            }
        }
        h(
            "sheet uvChannelMask animated not both UV0+UV1",
            p.sheet.as_ref().is_some_and(|s| s.uv_channel_mask != 0 && !(s.tiles_x < 2 && s.tiles_y < 2 && s.mode == 0) && s.uv_channel_mask & 3 != 3),
        );
        let rot3d = p.start_rotation_xy.is_some() || (p.shape_enabled && p.shape_align_to_direction) || p.rotation_separate;
        h("3D rotation (start xy / alignToDirection / separate rotation)", rot3d);
        h("trail lighting data", p.trail.as_ref().is_some_and(|t| t.lighting_data));
        h("ribbon attachRibbonsToTransform", p.trail.as_ref().is_some_and(|t| t.mode == 1 && t.attach_ribbons));
        h("trail texture mode > 3", p.trail.as_ref().is_some_and(|t| t.texture_mode > 3));
        let size3d = p.start_size_y.is_some() || p.size.as_ref().is_some_and(|c| c.separate) || p.size_by_speed.as_ref().is_some_and(|c| c.0.separate);
        h("ribbon with size-affects-width and 3D start size", p.trail.as_ref().is_some_and(|t| t.mode == 1 && t.size_affects_width) && size3d);
        h("ribbon with size-affects-width and start size state not 0|3", p.trail.as_ref().is_some_and(|t| t.mode == 1 && t.size_affects_width) && state(&p.start_size));
        h("shape texture on a shape type != 18", p.shape_enabled && p.shape_texture && p.shape_type != 18);
        if p.shape_texture {
            if let Some(t) = db.resolve(r.file, p.shape_texture_ref) {
                let t = crate::assets::components::Texture2D::read(db.data(t)).unwrap();
                h("shape texture not readable", !t.readable);
            }
        }
        if let Some(r) = rend {
            h("renderer render mode not 0|1|5", !matches!(r.render_mode, 0 | 1 | 5));
            let pivot = r.pivot.iter().any(|x| *x != 0.0) || size3d || p.sheet.as_ref().is_some_and(|s| s.mode == 1);
            h("stretched billboard with pivot/3D size/sprites, local space or camera velocity", r.render_mode == 1 && (pivot || p.space != 1 || r.stretch[0] != 0.0));
            h("renderer alignment not 0|1|2", !matches!(r.alignment, 0 | 1 | 2));
        } else {
            h("system without a ParticleSystemRenderer", true);
        }
        h("stop action 1", p.stop_action == 1);
        h("stop action 2|3", matches!(p.stop_action, 2 | 3));
        let miss = |k: &str, m: Option<String>| {
            if let Some(m) = m {
                hit(format!("{k}: {m}"), n)
            }
        };
        if let Some(g) = &p.color {
            let g = g.clone();
            miss("colour over lifetime", reach(move || { crate::particle_render::color_over_lifetime(&g, 0.0, 1, 0xffffffff); }));
        }
        if let Some((g, range)) = &p.color_by_speed {
            let (g, range) = (g.clone(), *range);
            miss("colour by speed", reach(move || { crate::particle_render::color_by_speed(&g, range, [0.0; 3], 0xffffffff); }));
        }
        if let Some(s) = &p.sheet {
            for alive in [0.0f32, 10.0, 50.0, 99.0] {
                for seed in [1u32, 77] {
                    let s = s.clone();
                    miss("sheet_index", reach(move || { crate::particle_render::sheet_index(&s, alive, 1.0, seed); }));
                }
            }
        }
        if let Some(t) = &p.trail {
            for g in [&t.color_over_lifetime, &t.color_over_trail] {
                let g = g.clone();
                miss("trail gradient", reach(move || { crate::particle_trails::gradient(&g, 0.0, 0.5); }));
            }
        }
        h("weighted curve key", all_curves(p).iter().any(|c| c.max.keys.iter().chain(&c.min.keys).any(|k| k.weighted_mode != 0)));
        h("collision enabled: not world 2D", p.collision.as_ref().is_some_and(|c| c.enabled && (c.kind != 1 || c.mode != 1)));
        h("collision enabled: sendCollisionMessages", p.collision.as_ref().is_some_and(|c| c.enabled && c.messages));
        h("collision enabled: colliderForce > 0", p.collision.as_ref().is_some_and(|c| c.enabled && c.collider_force > 0.0));
        h("collision enabled: plane type", p.collision.as_ref().is_some_and(|c| c.enabled && c.kind == 0));
        h("velocity: speedModifier not constant 1", p.velocity.as_ref().is_some_and(|v| v.speed_modifier.state != 0 || v.speed_modifier.scalar != 1.0));
        h("velocity: orbital/offset/radial not constant", p.velocity.as_ref().is_some_and(|v| v.orbital.iter().chain(&v.offset).chain([&v.radial]).any(|c| c.state != 0)));
        h("force: randomizePerFrame", p.force.as_ref().is_some_and(|f| f.randomize_per_frame));
        h("inherit initial with a curve", p.inherit.as_ref().is_some_and(|i| i.0 == 0 && !matches!(i.1.state, 0 | 3)));
        h("inherit current not TwoConstants", p.inherit.as_ref().is_some_and(|i| i.0 != 0 && i.1.state != 3));
        h("clamp magnitude state not 0|3", p.clamp.as_ref().is_some_and(|c| !matches!(c.magnitude.state, 0 | 3)));
        h("clamp drag state 2", p.clamp.as_ref().is_some_and(|c| c.drag.state == 2));
        h("noise quality not 0..2", p.noise.as_ref().is_some_and(|c| !(0..=2).contains(&c.quality)));
        h("noise remap", p.noise.as_ref().is_some_and(|c| c.remap_enabled));
        h("noise rotation amount with 3D rotation", p.noise.as_ref().is_some_and(|c| c.amounts[1].scalar != 0.0) && rot3d);
        h("shape alignToDirection", p.shape_enabled && p.shape_align_to_direction);
        h("sphere shape radiusThickness not 0|1", p.shape_enabled && p.shape_type == 0 && !matches!(p.shape_radius_thickness, 0.0 | 1.0));
        h(
            "sprite shape (20): texture/spawn mode/spread/placement",
            p.shape_enabled && p.shape_type == 20 && (p.shape_texture || p.shape_mesh_spawn.0 != 0 || p.shape_mesh_spawn.1 > 0.0 || !matches!(p.shape_placement, 1 | 2)),
        );
        h(
            "shape type not handled by ShapeModule::Start",
            p.shape_enabled
                && !(p.shape_type == 0 && p.shape_arc.mode == 0
                    || p.shape_type == 5
                    || p.shape_type == 10 && matches!(p.shape_arc.mode, 0 | 2)
                    || p.shape_type == 12 && matches!(p.shape_radius.mode, 0 | 2)
                    || p.shape_type == 18
                    || p.shape_type == 20),
        );
        h("start colour state 3", p.start_color.state == 3);
        h("ring buffer mode 2", p.ring_buffer_mode == 2);
        h("external forces influence list", p.external_forces.as_ref().is_some_and(|e| !e.influence.is_empty()));
    }
    for (fi, f) in db.files.iter().enumerate() {
        for o in f.objects.iter().filter(|o| o.class_id == 330) {
            let ff = ForceField::read(f.data(o)).unwrap();
            let n = format!("{:?}:{}", f.path.file_name().unwrap(), o.path_id);
            if ff.shape != 0 {
                hit("force field shape != 0".into(), &n);
            }
            if ff.direction.iter().any(|c| c.scalar != 0.0) {
                hit("force field direction".into(), &n);
            }
            if !ff.vector_field.is_null() && ff.vector_field_speed.scalar > 0.0 {
                hit("force field vector field".into(), &n);
            }
            hit("force fields".into(), &n);
        }
        for o in f.objects.iter().filter(|o| o.class_id == 182) {
            hit("wind zones".into(), &format!("{:?}:{}", f.path.file_name().unwrap(), o.path_id));
        }
        for o in f.objects.iter().filter(|o| o.class_id == 120) {
            let l = crate::assets::components::LineRendererData::read(f.data(o)).unwrap();
            let gn = crate::assets::components::game_object_of(f.data(o)).ok().and_then(|g| db.resolve(fi, g)).and_then(|g| db.game_object(g).ok()).map_or("?".into(), |g| g.name);
            let n = format!("{:?}:{}:{gn}", f.path.file_name().unwrap(), o.path_id);
            hit("line renderers".into(), &n);
            if l.corner_vertices != 0 {
                hit("line corner vertices".into(), &n);
            }
            if l.alignment != 0 {
                hit("line alignment not View".into(), &n);
            }
            if l.looped {
                hit("line looped".into(), &n);
            }
            if l.texture_mode > 3 {
                hit("line texture mode > 3".into(), &n);
            }
            if l.materials.iter().all(|m| m.path_id == 0) {
                hit("line without material".into(), &n);
            }
        }
    }
    // Runtime shape.texture writers: VaseDebrisBehaviour.SetSprite (SmallDebris, the vase's sprite texture).
    let mut monos = Vec::new();
    for (fi, f) in db.files.iter().enumerate() {
        for o in f.objects.iter().filter(|o| o.class_id == 114) {
            monos.push(ObjRef { file: fi, path_id: o.path_id });
        }
    }
    let mut vases = Vec::new();
    for r in monos {
        let Ok(Some(sc)) = db.script(r) else { continue };
        let n = format!("{}:{}", db.files[r.file].path.file_name().unwrap().to_string_lossy(), r.path_id);
        match sc.class_name.as_str() {
            "VaseDebrisBehaviour" => {
                hit("VaseDebrisBehaviour".into(), &n);
                let Ok(Some((_, _, v))) = db.mono(r) else { continue };
                let p = v.get("SmallDebris").and_then(|p| p.pptr()).unwrap();
                let Some(t) = db.resolve(r.file, p) else { continue };
                let go = crate::assets::components::game_object_of(db.data(t)).ok().and_then(|g| db.resolve(t.file, g));
                let rend = go.and_then(|g| db.game_object(g).ok()).and_then(|g| g.components.into_iter().filter_map(|c| db.resolve(t.file, c)).find(|c| db.class(*c) == 198));
                if let Some(ps) = rend.and_then(|c| ParticleSystem::read(db.data(c)).ok()) {
                    if !(ps.shape_enabled && ps.shape_type == 18) {
                        hit("VaseDebris SmallDebris shape is not a Rectangle".into(), &n);
                    }
                }
            }
            "VaseBehaviour" => vases.push((r, n)),
            _ => {}
        }
    }
    for (r, n) in vases {
        hit("VaseBehaviour".into(), &n);
        let mb = crate::assets::components::game_object_of(db.data(r)).ok().and_then(|g| db.resolve(r.file, g));
        let Some(go) = mb.and_then(|g| db.game_object(g).ok()) else { continue };
        for c in go.components.into_iter().filter_map(|c| db.resolve(r.file, c)).filter(|c| db.class(*c) == 212) {
            let sr = crate::assets::components::SpriteRenderer::read(db.data(c)).unwrap();
            let Some(sp) = db.resolve(c.file, sr.sprite) else {
                hit("VaseBehaviour sprite null".into(), &n);
                continue;
            };
            let sprite = db.sprite(sp).unwrap();
            match db.resolve(sp.file, sprite.texture) {
                None => hit("VaseBehaviour sprite without texture".into(), &n),
                Some(t) => {
                    let t = crate::assets::components::Texture2D::read(db.data(t)).unwrap();
                    if !t.readable {
                        hit("VaseBehaviour sprite texture not readable".into(), &format!("{n} {}", t.name));
                    }
                }
            }
        }
    }
    // Renderer / material / texture / audio / video settings behind the stops outside the particle files.
    for (fi, f) in db.files.iter().enumerate() {
        let file = f.path.file_name().unwrap().to_string_lossy().to_string();
        for o in &f.objects {
            let n = format!("{file}:{}", o.path_id);
            let r = ObjRef { file: fi, path_id: o.path_id };
            match o.class_id {
                82 => {
                    if let Some(m) = reach(|| {
                        let _ = crate::assets::components::AudioSource::read(f.data(o));
                    }) {
                        hit(format!("AudioSource: {m}"), &n);
                    }
                }
                21 => {
                    if let Ok(m) = crate::assets::components::Material::read(f.data(o)) {
                        if m.render_queue < 0 && !crate::shader_queues::SHADER_QUEUES.iter().any(|q| q.0 == db.material_shader_id(fi, m.shader)) && !m.shader.is_null() && m.shader.file == 0 {
                            hit("material whose shader has no queue".into(), &n);
                        }
                    }
                }
                28 => {
                    if let Ok(t) = crate::assets::components::Texture2D::read(f.data(o)) {
                        if t.wrap[..2].iter().any(|&w| w == 3) {
                            hit("texture wrap MirrorOnce".into(), &format!("{n} {}", t.name));
                        }
                        if t.wrap.contains(&2) {
                            hit("texture wrap Mirror".into(), &format!("{n} {}", t.name));
                        }
                    }
                }
                212 => {
                    if let Ok(sr) = crate::assets::components::SpriteRenderer::read(f.data(o)) {
                        if !crate::play::SORTING_LAYERS.contains(&(sr.sorting_layer_id as u32)) {
                            hit("sprite sortingLayerID not in TagManager".into(), &n);
                        }
                        if sr.draw_mode != 0 {
                            hit("sprite renderer draw mode Sliced/Tiled".into(), &n);
                        }
                    }
                }
                120 => {
                    if let Ok(l) = crate::assets::components::LineRendererData::read(f.data(o)) {
                        if !crate::play::SORTING_LAYERS.contains(&(l.layer as u32)) {
                            hit("line sortingLayerID not in TagManager".into(), &n);
                        }
                    }
                }
                199 => {
                    if let Ok(pr) = ParticleRenderer::read(f.data(o)) {
                        if !crate::play::SORTING_LAYERS.contains(&(pr.layer as u32)) {
                            hit("particle renderer sortingLayerID not in TagManager".into(), &n);
                        }
                    }
                }
                328 => {
                    hit("VideoPlayer".into(), &n);
                    let go = crate::assets::components::game_object_of(f.data(o)).ok().and_then(|g| db.resolve(fi, g)).and_then(|g| db.game_object(g).ok());
                    let srs: Vec<_> = go.iter().flat_map(|g| g.components.iter()).filter_map(|c| db.resolve(fi, *c)).filter(|c| db.class(*c) == 212).collect();
                    if srs.is_empty() {
                        hit("VideoPlayer on a GameObject without a SpriteRenderer".into(), &n);
                    }
                    for c in srs {
                        let sr = crate::assets::components::SpriteRenderer::read(db.data(c)).unwrap();
                        match sr.materials.first().and_then(|m| db.resolve(c.file, *m)) {
                            None => hit("VideoPlayer renderer without a material".into(), &n),
                            Some(m) => {
                                let m = crate::assets::components::Material::read(db.data(m)).unwrap();
                                if (m.float("_SrcBlend"), m.float("_DstBlend")) != (Some(5.0), Some(1.0)) {
                                    hit("VideoPlayer material not SrcAlpha One".into(), &n);
                                }
                            }
                        }
                    }
                    let _ = r;
                }
                _ => {}
            }
        }
    }
    let _ = std::panic::take_hook();
    println!("SCAN {total:5} | systems | ");
    for (k, (n, ex)) in out.borrow().iter() {
        println!("SCAN {n:5} | {k} | {}", ex.join(", "));
    }
}
